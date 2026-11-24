//! Owned document state and the single mutation portal shared by executors.
use crate::{
    file::FileBinding,
    motion::{LineChange, LineIndex},
    vim::{InputError, SelectionKind},
    EditError, TextBuffer,
};
use std::{collections::VecDeque, fmt, io, ops::Range, path::Path};

/// Recoverable editor/session failure. Failed writes never mark text saved.
#[derive(Debug)]
pub enum EditorError {
    /// Invalid UTF-8 edit coordinates.
    Edit(EditError),
    /// Modal parser rejected input.
    Input(InputError),
    /// Filesystem operation failed.
    Io(io::Error),
    /// On-disk contents or identity changed since open/save.
    FileChanged,
    /// Final path entry is a symbolic link; saving it is refused.
    SymbolicLink,
    /// An untitled document needs a path before saving.
    NoPath,
    /// Closing would discard unsaved text.
    UnsavedChanges,
    /// The session has already closed.
    Closed,
    /// There is no previous search to repeat.
    NoSearch,
    /// A literal search found no match.
    SearchNotFound(String),
    /// The unnamed register has no contents.
    EmptyRegister,
    /// A requested operation is not yet supported.
    Unsupported(&'static str),
    /// Repetition would allocate too much text in one operation.
    EditTooLarge,
}

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Edit(e) => fmt::Display::fmt(e, f),
            Self::Input(e) => fmt::Display::fmt(e, f),
            Self::Io(e) => fmt::Display::fmt(e, f),
            Self::FileChanged => {
                f.write_str("file changed externally; reopen or reconcile before saving")
            }
            Self::SymbolicLink => f.write_str("symbolic-link file entries are not supported"),
            Self::NoPath => {
                f.write_str("untitled document has no file name; save it with :w <file>")
            }
            Self::UnsavedChanges => {
                f.write_str("unsaved changes; save first or explicitly use :q!")
            }
            Self::Closed => f.write_str("editor session is closed"),
            Self::NoSearch => f.write_str("no previous search"),
            Self::SearchNotFound(pattern) => write!(f, "literal search not found: {pattern}"),
            Self::EmptyRegister => f.write_str("unnamed register is empty"),
            Self::Unsupported(message) => f.write_str(message),
            Self::EditTooLarge => f.write_str("repeated edit exceeds the 64 MiB operation budget"),
        }
    }
}
impl std::error::Error for EditorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<EditError> for EditorError {
    fn from(e: EditError) -> Self {
        Self::Edit(e)
    }
}
impl From<InputError> for EditorError {
    fn from(e: InputError) -> Self {
        Self::Input(e)
    }
}
impl From<io::Error> for EditorError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Zero-based logical coordinates. Tabs/wide glyphs each count as one scalar.
/// These are neither grapheme nor screen-cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Logical line number; LF and CRLF are supported.
    pub line: usize,
    /// Number of Unicode scalar values before the cursor on this line.
    pub scalar_column: usize,
}

/// Inclusive Visual endpoints stored as validated UTF-8 byte boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Stable selection origin.
    pub anchor: usize,
    /// Current moving endpoint.
    pub head: usize,
    /// Character or whole-line geometry.
    pub kind: SelectionKind,
}

/// Unnamed register contents; clipboard integration is not required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    /// Exact captured text, including existing line endings.
    pub text: String,
    /// Whether paste inserts characters or complete lines.
    pub kind: SelectionKind,
}

#[derive(Debug)]
struct CursorChange {
    before: CursorState,
    after: CursorState,
    lines: Vec<LineChange>,
}
#[derive(Debug, Clone, Copy)]
struct CursorState {
    offset: usize,
    preferred: Option<usize>,
}
#[derive(Debug)]
struct Group {
    before: CursorState,
    revision: u64,
    lines: Vec<LineChange>,
}
#[derive(Debug, Clone)]
pub(crate) struct SearchEntry {
    pub pattern: String,
    pub backward: bool,
}

/// Document data independent of the parser's single source of truth for mode.
/// Mutable buffer access is private so text and cursor history cannot diverge.
#[derive(Debug)]
pub struct EditorState {
    pub(crate) buffer: TextBuffer,
    pub(crate) lines: LineIndex,
    pub(crate) cursor: usize,
    pub(crate) preferred: Option<usize>,
    pub(crate) selection: Option<Selection>,
    pub(crate) unnamed: Option<Register>,
    pub(crate) searches: VecDeque<SearchEntry>,
    pub(crate) file: Option<FileBinding>,
    pub(crate) eol: &'static str,
    pub(crate) closed: bool,
    pub(crate) indent: crate::indent::IndentStyle,
    initial_unsaved: bool,
    undo: VecDeque<CursorChange>,
    redo: Vec<CursorChange>,
    group: Option<Group>,
}

impl EditorState {
    pub(crate) fn new(text: String, file: Option<FileBinding>, loaded: bool) -> Self {
        let initial_unsaved = !loaded && !text.is_empty();
        let eol = text
            .find('\n')
            .map(|i| {
                if i > 0 && text.as_bytes()[i - 1] == b'\r' {
                    "\r\n"
                } else {
                    "\n"
                }
            })
            .unwrap_or("\n");
        let buffer = TextBuffer::new(text);
        let lines = LineIndex::new(&buffer);
        Self {
            buffer,
            lines,
            cursor: 0,
            preferred: None,
            selection: None,
            unnamed: None,
            searches: VecDeque::new(),
            file,
            eol,
            closed: false,
            indent: crate::indent::IndentStyle::default(),
            initial_unsaved,
            undo: VecDeque::new(),
            redo: Vec::new(),
            group: None,
        }
    }
    /// Immutable buffer view for renderers and automation.
    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }
    /// Validated byte offset of the cursor.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    /// Desired Unicode scalar column retained through shorter lines.
    pub fn preferred_column(&self) -> Option<usize> {
        self.preferred
    }
    /// Bound path, or None for an untitled document.
    pub fn path(&self) -> Option<&Path> {
        self.file.as_ref().map(FileBinding::path)
    }
    /// Indentation settings used by Enter, typed closers and Tab/Shift+Tab.
    pub fn indent(&self) -> crate::indent::IndentStyle {
        self.indent
    }
    /// Line ending used for new Enter/open-line operations; existing bytes remain intact.
    pub fn line_ending(&self) -> &str {
        self.eol
    }
    /// Logical Unicode scalar coordinates, calculated without copying text.
    pub fn position(&self) -> Result<Position, EditorError> {
        Ok(Position {
            line: self.lines.row(self.cursor),
            scalar_column: self.lines.column(self.cursor, &self.buffer)?,
        })
    }
    /// Number of logical lines, including an empty line after a terminal LF.
    pub fn line_count(&self) -> usize {
        self.lines.count()
    }
    /// UTF-8 byte range of a logical line, excluding its LF/CRLF terminator.
    /// Uses the existing line index without materializing the document.
    /// An empty final line has an empty range; out-of-range lines return None.
    pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
        (line < self.lines.count())
            .then(|| self.lines.start(line)..self.lines.end(line, &self.buffer))
    }
    /// Current inclusive Visual endpoints, if any.
    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }
    /// Unnamed register contents, if initialized.
    pub fn register(&self) -> Option<&Register> {
        self.unnamed.as_ref()
    }
    /// Whether closing requires an explicit save/discard decision.
    pub fn is_modified(&self) -> bool {
        self.initial_unsaved || self.buffer.is_modified()
    }
    /// Whether a successful quit already closed this session.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    /// Re-open a session whose quit was intercepted by the host (e.g. unsaved other tabs).
    pub fn clear_closed(&mut self) {
        self.closed = false;
    }
    /// Previous literal searches, newest last, bounded to 32 entries.
    pub fn search_history(&self) -> impl Iterator<Item = (&str, bool)> {
        self.searches
            .iter()
            .map(|entry| (entry.pattern.as_str(), entry.backward))
    }
    fn cursor_state(&self) -> CursorState {
        CursorState {
            offset: self.cursor,
            preferred: self.preferred,
        }
    }
    pub(crate) fn begin_group(&mut self) {
        if self.group.is_none() {
            self.group = Some(Group {
                before: self.cursor_state(),
                revision: self.buffer.revision(),
                lines: Vec::new(),
            });
            self.buffer.begin_edit_group();
        }
    }
    fn remember(&mut self, change: CursorChange) {
        self.undo.push_back(change);
        if self.undo.len() > self.buffer.history_limit() {
            self.undo.pop_front();
        }
    }
    pub(crate) fn finish_group(&mut self) {
        self.buffer.end_edit_group();
        if let Some(group) = self.group.take() {
            if group.revision != self.buffer.revision() {
                self.remember(CursorChange {
                    before: group.before,
                    after: self.cursor_state(),
                    lines: group.lines,
                });
            }
        }
    }
    pub(crate) fn discard_empty_group(&mut self) {
        if self
            .group
            .as_ref()
            .is_some_and(|group| group.revision == self.buffer.revision())
        {
            self.group = None;
            self.buffer.end_edit_group();
        }
    }
    pub(crate) fn replace(
        &mut self,
        range: Range<usize>,
        text: &str,
        cursor: usize,
        insert: bool,
    ) -> Result<(), EditorError> {
        // Validate both old range and future cursor before changing either history.
        let _ = self.buffer.chunks_in(range.clone())?;
        let new_len = self.buffer.byte_len() - (range.end - range.start) + text.len();
        if cursor > new_len {
            return Err(EditError::InvalidRange.into());
        }
        if cursor < range.start {
            let _ = self.buffer.chunks_in(cursor..cursor)?;
        } else if cursor <= range.start + text.len() {
            if !text.is_char_boundary(cursor - range.start) {
                return Err(EditError::InvalidBoundary.into());
            }
        } else {
            let old = cursor - text.len() + (range.end - range.start);
            let _ = self.buffer.chunks_in(old..old)?;
        }
        let before = self.cursor_state();
        let revision = self.buffer.revision();
        let lines = self.lines.change(range.clone(), text);
        self.buffer.replace(range, text)?;
        if revision != self.buffer.revision() {
            self.lines.apply(&lines, true);
        }
        self.cursor = self.lines.clamp(cursor, insert, &self.buffer)?;
        self.preferred = None;
        if revision != self.buffer.revision() {
            self.redo.clear();
            if let Some(group) = &mut self.group {
                if !group
                    .lines
                    .last_mut()
                    .is_some_and(|last| last.append(&lines))
                {
                    group.lines.push(lines);
                }
            } else {
                self.remember(CursorChange {
                    before,
                    after: self.cursor_state(),
                    lines: vec![lines],
                });
            }
        }
        Ok(())
    }
    pub(crate) fn restore(&mut self, redo: bool) -> Result<(), EditorError> {
        self.finish_group();
        if redo {
            let Some(change) = self.redo.pop() else {
                return Ok(());
            };
            if !self.buffer.redo() {
                return Err(EditorError::Unsupported(
                    "cursor/text redo history diverged",
                ));
            }
            for lines in &change.lines {
                self.lines.apply(lines, true);
            }
            self.cursor = change.after.offset;
            self.preferred = change.after.preferred;
            self.undo.push_back(change);
        } else {
            let Some(change) = self.undo.pop_back() else {
                return Ok(());
            };
            if !self.buffer.undo() {
                return Err(EditorError::Unsupported(
                    "cursor/text undo history diverged",
                ));
            }
            for lines in change.lines.iter().rev() {
                self.lines.apply(lines, false);
            }
            self.cursor = change.before.offset;
            self.preferred = change.before.preferred;
            self.redo.push(change);
        }
        self.cursor = self.lines.clamp(self.cursor, false, &self.buffer)?;
        self.selection = None;
        Ok(())
    }
    pub(crate) fn save(&mut self) -> Result<crate::file::SaveReport, EditorError> {
        self.finish_group();
        let report = self
            .file
            .as_mut()
            .ok_or(EditorError::NoPath)?
            .save(&self.buffer)?;
        self.buffer.mark_saved();
        self.initial_unsaved = false;
        Ok(report)
    }
    /// Write to a path that must not exist yet, then bind the document to it.
    /// On failure the previous binding (and unsaved state) is unchanged.
    pub(crate) fn save_as(
        &mut self,
        path: &std::path::Path,
    ) -> Result<crate::file::SaveReport, EditorError> {
        self.finish_group();
        let mut binding = FileBinding::create(path)?;
        let report = binding.save(&self.buffer)?;
        self.file = Some(binding);
        self.buffer.mark_saved();
        self.initial_unsaved = false;
        Ok(report)
    }
}
