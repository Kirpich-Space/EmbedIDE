//! Frontend-independent UTF-8 editing with a piece table and bounded undo.
//!
//! Offsets are bytes, suitable for compiler and Tree-sitter adapters. All edit
//! endpoints must be Unicode scalar boundaries. Grapheme navigation, rendering,
//! file I/O and modal input are separate responsibilities.
//!
//! Edits and undo do not rebuild or clone the full document. Metadata uses a flat
//! vector: edits and boundary lookup are O(pieces), not logarithmic. History is
//! bounded by transaction count; the insertion store lives with the buffer.

#[cfg(not(unix))]
compile_error!(
    "EmbedIDE supports Linux and FreeBSD only (Unix systems); Windows is not supported."
);

use std::{collections::VecDeque, fmt, io, ops::Range};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

pub mod asm;
pub mod completion;
mod document;
mod executor;
pub mod file;
pub mod indent;
mod motion;
mod search;
mod session;
pub mod syntax;
pub mod vim;

pub use document::{EditorError, EditorState, Position, Register, Selection};
pub use session::{Editor, Effect};

/// Why an edit or slice was rejected. Rejected operations leave state unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// Range is reversed or extends beyond the document.
    InvalidRange,
    /// An endpoint splits a UTF-8 encoded scalar value.
    InvalidBoundary,
    /// The buffer has exhausted its unique revision identifiers.
    RevisionExhausted,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRange => "range is reversed or outside the document",
            Self::InvalidBoundary => "range endpoint splits a UTF-8 character",
            Self::RevisionExhausted => "buffer revision identifiers exhausted",
        })
    }
}

impl std::error::Error for EditError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Original,
    Added,
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    source: Source,
    start: usize,
    len: usize,
}

#[derive(Debug, Clone)]
struct State {
    pieces: Vec<Piece>,
    len: usize,
    revision: u64,
}

/// An owned UTF-8 document with transaction-level undo and redo.
///
/// Replacements are separate transactions unless explicitly grouped. BOMs, CRLF
/// and trailing newlines are preserved verbatim. Undo retains piece descriptors,
/// not full text snapshots. Dropping a buffer releases all its storage.
#[derive(Debug)]
pub struct TextBuffer {
    original: String,
    added: String,
    state: State,
    undo: VecDeque<State>,
    redo: Vec<State>,
    history_limit: usize,
    next_revision: u64,
    saved_revision: u64,
    edit_group: Option<bool>,
}

impl TextBuffer {
    /// Own text without copying it, retaining up to 128 undo transactions.
    pub fn new(text: String) -> Self {
        Self::with_history_limit(text, 128)
    }

    /// Own text with a chosen maximum number of undo transactions.
    /// A zero limit disables history but still tracks unsaved changes.
    pub fn with_history_limit(text: String, history_limit: usize) -> Self {
        let len = text.len();
        let pieces = if len == 0 {
            Vec::new()
        } else {
            vec![Piece {
                source: Source::Original,
                start: 0,
                len,
            }]
        };
        Self {
            original: text,
            added: String::new(),
            state: State {
                pieces,
                len,
                revision: 0,
            },
            undo: VecDeque::new(),
            redo: Vec::new(),
            history_limit,
            next_revision: 1,
            saved_revision: 0,
            edit_group: None,
        }
    }

    /// Document length in UTF-8 bytes.
    pub fn byte_len(&self) -> usize {
        self.state.len
    }

    /// Unique current revision, including undo/redo restorations.
    pub fn revision(&self) -> u64 {
        self.state.revision
    }

    /// Maximum retained undo transactions.
    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// Read a byte without treating its offset as a character coordinate.
    pub fn byte_at(&self, offset: usize) -> Option<u8> {
        let mut base = 0;
        for chunk in self.chunks() {
            if offset < base + chunk.len() {
                return chunk.as_bytes().get(offset - base).copied();
            }
            base += chunk.len();
        }
        None
    }

    /// Borrow fragments of a validated UTF-8 range, without allocating text.
    pub fn chunks_in(&self, range: Range<usize>) -> Result<impl Iterator<Item = &str>, EditError> {
        self.validate(&range)?;
        let mut base = 0;
        Ok(self.chunks().filter_map(move |chunk| {
            let start = range.start.max(base);
            let end = range.end.min(base + chunk.len());
            let local = base;
            base += chunk.len();
            (start < end).then(|| &chunk[start - local..end - local])
        }))
    }

    /// Read the Unicode scalar at a validated byte boundary, or None at EOF.
    pub fn char_at(&self, offset: usize) -> Result<Option<char>, EditError> {
        Ok(self
            .chunks_in(offset..self.byte_len())?
            .find_map(|chunk| chunk.chars().next()))
    }

    /// Previous Unicode scalar boundary; zero remains zero.
    pub fn previous_boundary(&self, offset: usize) -> Result<usize, EditError> {
        self.validate(&(offset..offset))?;
        let mut base = 0;
        for chunk in self.chunks() {
            if offset <= base + chunk.len() && offset > base {
                return Ok(base
                    + chunk[..offset - base]
                        .char_indices()
                        .next_back()
                        .map(|(index, _)| index)
                        .unwrap_or(0));
            }
            base += chunk.len();
        }
        Ok(0)
    }

    /// Next Unicode scalar boundary; EOF remains EOF.
    pub fn next_boundary(&self, offset: usize) -> Result<usize, EditError> {
        Ok(offset + self.char_at(offset)?.map(char::len_utf8).unwrap_or(0))
    }

    fn grapheme_chunk(&self, offset: usize, left: bool) -> Option<(&str, usize)> {
        let mut base = 0;
        for chunk in self.chunks() {
            let end = base + chunk.len();
            if offset < end || (left && offset == end && !chunk.is_empty()) {
                return Some((chunk, base));
            }
            base = end;
        }
        None
    }
    fn grapheme_boundary(&self, offset: usize, forward: bool) -> Result<usize, EditError> {
        self.validate(&(offset..offset))?;
        if forward && offset == self.byte_len() || !forward && offset == 0 {
            return Ok(offset);
        }
        let mut cursor = GraphemeCursor::new(offset, self.byte_len(), true);
        let (mut chunk, mut base) = self
            .grapheme_chunk(offset, !forward)
            .ok_or(EditError::InvalidBoundary)?;
        loop {
            let boundary = if forward {
                cursor.next_boundary(chunk, base)
            } else {
                cursor.prev_boundary(chunk, base)
            };
            match boundary {
                Ok(boundary) => {
                    return Ok(boundary.unwrap_or(if forward { self.byte_len() } else { 0 }))
                }
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, base) = self
                        .grapheme_chunk(base + chunk.len(), false)
                        .ok_or(EditError::InvalidBoundary)?;
                }
                Err(GraphemeIncomplete::PrevChunk) => {
                    (chunk, base) = self
                        .grapheme_chunk(base, true)
                        .ok_or(EditError::InvalidBoundary)?;
                }
                Err(GraphemeIncomplete::PreContext(end)) => {
                    let (context, start) = self
                        .grapheme_chunk(end, true)
                        .ok_or(EditError::InvalidBoundary)?;
                    cursor.provide_context(&context[..end - start], start);
                }
                Err(GraphemeIncomplete::InvalidOffset) => return Err(EditError::InvalidBoundary),
            }
        }
    }
    /// Next extended grapheme boundary across piece-table fragments, without flattening.
    pub fn next_grapheme(&self, offset: usize) -> Result<usize, EditError> {
        self.grapheme_boundary(offset, true)
    }
    /// Previous extended grapheme boundary, including CRLF as one unit.
    pub fn previous_grapheme(&self, offset: usize) -> Result<usize, EditError> {
        self.grapheme_boundary(offset, false)
    }
    /// Snap a valid scalar offset to the start of its containing grapheme.
    pub fn grapheme_start(&self, offset: usize) -> Result<usize, EditError> {
        self.validate(&(offset..offset))?;
        if offset == 0 {
            return Ok(0);
        }
        let previous = self.previous_grapheme(offset)?;
        if self.next_grapheme(previous)? == offset {
            Ok(offset)
        } else {
            Ok(previous)
        }
    }

    /// Whether the document contains no text.
    pub fn is_empty(&self) -> bool {
        self.state.len == 0
    }

    /// Whether the current revision differs from the last saved revision.
    /// Different edit paths to identical text can still have different revisions.
    pub fn is_modified(&self) -> bool {
        self.state.revision != self.saved_revision
    }

    /// Mark this revision saved, only after the caller's persistence succeeds.
    pub fn mark_saved(&mut self) {
        self.end_edit_group();
        self.saved_revision = self.state.revision;
    }

    /// Group subsequent replacements into one undo transaction, for example an
    /// Insert-mode session. Calling this again keeps the existing group open.
    /// An empty group creates no undo entry. Call `end_edit_group` on completion.
    pub fn begin_edit_group(&mut self) {
        if self.edit_group.is_none() {
            self.edit_group = Some(false);
        }
    }

    /// Finish a group. Undo, redo and marking saved also finish the open group.
    /// Replacements outside a group each create their own undo transaction.
    pub fn end_edit_group(&mut self) {
        self.edit_group = None;
    }

    fn source(&self, source: Source) -> &str {
        match source {
            Source::Original => &self.original,
            Source::Added => &self.added,
        }
    }

    /// Borrow document fragments in order, without allocating a full copy.
    pub fn chunks(&self) -> impl Iterator<Item = &str> {
        self.state.pieces.iter().map(|piece| {
            let source = self.source(piece.source);
            &source[piece.start..piece.start + piece.len]
        })
    }

    /// Materialize text for consumers that need a contiguous string.
    /// Rendering should use fragments or bounded slices instead.
    pub fn contents(&self) -> String {
        let mut text = String::with_capacity(self.byte_len());
        for chunk in self.chunks() {
            text.push_str(chunk);
        }
        text
    }

    /// Stream exact bytes to a caller-owned writer, propagating I/O errors.
    /// This does not mark the revision saved or provide atomic file replacement.
    pub fn write_to(&self, writer: &mut impl io::Write) -> io::Result<()> {
        for chunk in self.chunks() {
            writer.write_all(chunk.as_bytes())?;
        }
        Ok(())
    }

    fn is_boundary(&self, offset: usize) -> bool {
        if offset == self.byte_len() {
            return true;
        }
        let mut base = 0;
        for chunk in self.chunks() {
            if offset < base + chunk.len() {
                return chunk.is_char_boundary(offset - base);
            }
            base += chunk.len();
        }
        false
    }

    fn validate(&self, range: &Range<usize>) -> Result<(), EditError> {
        if range.start > range.end || range.end > self.byte_len() {
            return Err(EditError::InvalidRange);
        }
        if !self.is_boundary(range.start) || !self.is_boundary(range.end) {
            return Err(EditError::InvalidBoundary);
        }
        Ok(())
    }

    fn push_piece(pieces: &mut Vec<Piece>, piece: Piece) {
        if piece.len == 0 {
            return;
        }
        if let Some(last) = pieces.last_mut() {
            if last.source == piece.source && last.start + last.len == piece.start {
                last.len += piece.len;
                return;
            }
        }
        pieces.push(piece);
    }

    fn copy_range(&self, range: Range<usize>, pieces: &mut Vec<Piece>) {
        let mut base = 0;
        for piece in &self.state.pieces {
            if base >= range.end {
                break;
            }
            let start = range.start.max(base);
            let end = range.end.min(base + piece.len);
            if start < end {
                Self::push_piece(
                    pieces,
                    Piece {
                        source: piece.source,
                        start: piece.start + start - base,
                        len: end - start,
                    },
                );
            }
            base += piece.len;
        }
    }

    /// Copy a validated byte range into a string.
    pub fn slice(&self, range: Range<usize>) -> Result<String, EditError> {
        self.validate(&range)?;
        let mut pieces = Vec::new();
        self.copy_range(range.clone(), &mut pieces);
        let mut text = String::with_capacity(range.end - range.start);
        for piece in pieces {
            text.push_str(&self.source(piece.source)[piece.start..piece.start + piece.len]);
        }
        Ok(text)
    }

    /// Replace a byte range, joining the open group if present.
    /// An empty insertion is a no-op.
    /// A new edit after undo discards the redo branch.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), EditError> {
        self.validate(&range)?;
        if range.is_empty() && text.is_empty() {
            return Ok(());
        }
        if range.end - range.start == text.len()
            && self
                .chunks_in(range.clone())?
                .flat_map(str::bytes)
                .eq(text.bytes())
        {
            return Ok(());
        }
        let next_revision = self
            .next_revision
            .checked_add(1)
            .ok_or(EditError::RevisionExhausted)?;
        let mut pieces = Vec::new();
        self.copy_range(0..range.start, &mut pieces);
        Self::push_piece(
            &mut pieces,
            Piece {
                source: Source::Added,
                start: self.added.len(),
                len: text.len(),
            },
        );
        self.copy_range(range.end..self.byte_len(), &mut pieces);
        let len = self.byte_len() - (range.end - range.start) + text.len();
        self.added.push_str(text);
        let previous = std::mem::replace(
            &mut self.state,
            State {
                pieces,
                len,
                revision: self.next_revision,
            },
        );
        self.next_revision = next_revision;
        if self.history_limit > 0 && self.edit_group != Some(true) {
            self.undo.push_back(previous);
            if self.undo.len() > self.history_limit {
                self.undo.pop_front();
            }
        }
        if let Some(changed) = &mut self.edit_group {
            *changed = true;
        }
        self.redo.clear();
        Ok(())
    }

    /// Whether an undo transaction is available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether a redo transaction is available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Restore the previous revision; return false when history is empty.
    pub fn undo(&mut self) -> bool {
        self.end_edit_group();
        match self.undo.pop_back() {
            Some(previous) => {
                self.redo.push(std::mem::replace(&mut self.state, previous));
                true
            }
            None => false,
        }
    }

    /// Restore the next revision; return false when history is empty.
    pub fn redo(&mut self) -> bool {
        self.end_edit_group();
        match self.redo.pop() {
            Some(next) => {
                self.undo
                    .push_back(std::mem::replace(&mut self.state, next));
                true
            }
            None => false,
        }
    }
}
