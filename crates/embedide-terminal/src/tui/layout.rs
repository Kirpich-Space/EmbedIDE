//! Terminal-only byte/grapheme/cell mapping; document coordinates remain bytes.
use crate::FrontendError;
use embedide_editor::{
    syntax::{Highlight, Highlighter, Span},
    vim::SelectionKind,
    Editor, EditorState, Selection,
};
use std::{collections::VecDeque, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const CACHE_BYTES: usize = 4 * 1024 * 1024;
const UNICODE_LINE_LIMIT: usize = 16 * 1024;
const TAB_STOP: usize = 4;

/// One contiguous terminal run with a uniform selection attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Sanitized text, safe for terminal output.
    pub text: String,
    /// Whether the run should use inverse video.
    pub selected: bool,
    /// Syntax category (never set on selected runs: selection stays uniform).
    pub style: Option<Highlight>,
    /// Colours and attributes a program running in the integrated terminal
    /// asked for (never set on editor text).
    pub cell: Option<CellStyle>,
}

/// SGR state of terminal-panel output. `None` colours keep the theme's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellStyle {
    /// Foreground colour.
    pub fg: Option<crossterm::style::Color>,
    /// Background colour.
    pub bg: Option<crossterm::style::Color>,
    /// Bold.
    pub bold: bool,
    /// Dim (faint).
    pub dim: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Inverse video (selection bars in `less`, `fzf`, `htop`, …).
    pub inverse: bool,
}

/// A bounded screen row. Rows never contain source control characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    /// Consecutive text/style runs.
    pub runs: Vec<Run>,
}
impl Row {
    pub(crate) fn push(&mut self, text: &str, selected: bool) {
        self.push_styled(text, selected, None);
    }
    pub(crate) fn push_styled(&mut self, text: &str, selected: bool, style: Option<Highlight>) {
        self.push_attrs(text, selected, style, None);
    }
    /// Append text with every attribute a run can carry, merging with the
    /// previous run when they are identical.
    pub(crate) fn push_attrs(
        &mut self,
        text: &str,
        selected: bool,
        style: Option<Highlight>,
        cell: Option<CellStyle>,
    ) {
        let (style, cell) = if selected {
            (None, None)
        } else {
            (style, cell)
        };
        if let Some(last) = self
            .runs
            .last_mut()
            .filter(|last| last.selected == selected && last.style == style && last.cell == cell)
        {
            last.text.push_str(text);
        } else if !text.is_empty() {
            self.runs.push(Run {
                text: text.into(),
                selected,
                style,
                cell,
            });
        }
    }
}

#[derive(Debug)]
struct Token {
    bytes: Range<usize>,
    column: usize,
    width: usize,
    text: String,
}
#[derive(Debug)]
enum Layout {
    Unicode {
        tokens: Vec<Token>,
        end: usize,
        width: usize,
    },
    Escaped {
        range: Range<usize>,
        tab_width: usize,
    },
}

pub(crate) fn unsafe_char(ch: char) -> bool {
    ch.is_control()
        || matches!(ch, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn grapheme_text(grapheme: &str, column: usize, tab_stop: usize) -> (String, usize) {
    if grapheme == "\t" {
        let width = tab_stop - column % tab_stop;
        return (" ".repeat(width), width);
    }
    if grapheme.chars().any(unsafe_char) {
        let text: String = grapheme.chars().flat_map(char::escape_default).collect();
        let width = text.len();
        return (text, width);
    }
    let width = UnicodeWidthStr::width(grapheme);
    if width == 0 {
        let text: String = grapheme.chars().flat_map(char::escape_unicode).collect();
        let width = text.len();
        (text, width)
    } else {
        (grapheme.into(), width)
    }
}

fn scalar_width(ch: char, column: usize, tab_width: usize) -> usize {
    if ch == '\t' {
        tab_width - column % tab_width
    } else if ch.is_ascii() && !ch.is_control() {
        1
    } else {
        ch.escape_unicode().count()
    }
}
fn scalar_text(ch: char, column: usize, tab_width: usize) -> String {
    if ch == '\t' {
        " ".repeat(scalar_width(ch, column, tab_width))
    } else if ch.is_ascii() && !ch.is_control() {
        ch.to_string()
    } else {
        ch.escape_unicode().collect()
    }
}

impl Layout {
    fn from_text(text: &str, base: usize, tab_width: usize) -> Self {
        let mut column = 0;
        let tokens = text
            .grapheme_indices(true)
            .map(|(offset, grapheme)| {
                let (text, width) = grapheme_text(grapheme, column, tab_width);
                let token = Token {
                    bytes: base + offset..base + offset + grapheme.len(),
                    column,
                    width,
                    text,
                };
                column += width;
                token
            })
            .collect();
        Self::Unicode {
            tokens,
            end: base + text.len(),
            width: column,
        }
    }
    fn memory(&self) -> usize {
        match self {
            Self::Unicode { tokens, .. } => {
                tokens.capacity() * std::mem::size_of::<Token>()
                    + tokens
                        .iter()
                        .map(|token| token.text.capacity())
                        .sum::<usize>()
            }
            Self::Escaped { .. } => std::mem::size_of::<Self>(),
        }
    }
    /// Byte offset of the grapheme drawn at display `column` (the line end
    /// past the last one). Escaped long lines map to their start.
    fn byte_at(&self, column: usize) -> usize {
        match self {
            Self::Unicode { tokens, end, .. } => tokens
                .iter()
                .find(|token| column < token.column + token.width.max(1))
                .map_or(*end, |token| token.bytes.start),
            Self::Escaped { range, .. } => range.start,
        }
    }
    fn cursor(&self, state: &EditorState, byte: usize) -> Result<(usize, usize), FrontendError> {
        match self {
            Self::Unicode { tokens, end, width } => {
                if byte >= *end {
                    return Ok((*width, 1));
                }
                let index = tokens.partition_point(|token| token.bytes.end <= byte);
                Ok(tokens
                    .get(index)
                    .map_or((0, 1), |token| (token.column, token.width)))
            }
            Self::Escaped { range, tab_width } => {
                let mut column = 0;
                let mut offset = range.start;
                for ch in state
                    .buffer()
                    .chunks_in(range.clone())?
                    .flat_map(str::chars)
                {
                    let width = scalar_width(ch, column, *tab_width);
                    if offset >= byte {
                        return Ok((column, width));
                    }
                    column += width;
                    offset += ch.len_utf8();
                }
                Ok((column, 1))
            }
        }
    }
    fn paint(
        &self,
        state: &EditorState,
        left: usize,
        width: usize,
        selection: Option<Selection>,
        spans: &[Span],
        row: &mut Row,
    ) -> Result<(), FrontendError> {
        // Tokens arrive in byte order, so one forward cursor finds each style.
        let mut next_span = 0;
        let mut style_at = |byte: usize| {
            while spans.get(next_span).is_some_and(|s| s.range.end <= byte) {
                next_span += 1;
            }
            spans
                .get(next_span)
                .filter(|s| s.range.start <= byte)
                .map(|s| s.kind)
        };
        let selected = |range: &Range<usize>| {
            selection.is_some_and(|s| {
                let low = s.anchor.min(s.head);
                let high = s.anchor.max(s.head);
                range.start <= high && range.end > low
            })
        };
        let whole_line = selection.filter(|s| s.kind == SelectionKind::Line);
        let line_selected = whole_line.is_some_and(|s| {
            let range = match self {
                Self::Unicode { tokens, end, .. } => tokens
                    .first()
                    .map_or(*end..*end, |token| token.bytes.start..*end),
                Self::Escaped { range, .. } => range.clone(),
            };
            range.start <= s.anchor.max(s.head) && range.end >= s.anchor.min(s.head)
        });
        let mut emit = |text: &str, column: usize, cells: usize, range: &Range<usize>| {
            let start = column.max(left);
            let end = (column + cells).min(left + width);
            if start >= end {
                return;
            }
            let highlight = if whole_line.is_some() {
                line_selected
            } else {
                selected(range)
            };
            let style = style_at(range.start);
            if start == column && end == column + cells {
                row.push_styled(text, highlight, style);
            } else {
                row.push_styled(&" ".repeat(end - start), highlight, style);
            }
        };
        match self {
            Self::Unicode { tokens, .. } => {
                for token in tokens
                    .iter()
                    .skip_while(|token| token.column + token.width <= left)
                {
                    if token.column >= left + width {
                        break;
                    }
                    emit(&token.text, token.column, token.width, &token.bytes);
                }
                if tokens.is_empty() && line_selected && width > 0 {
                    row.push(" ", true);
                }
            }
            Self::Escaped { range, tab_width } => {
                let mut offset = range.start;
                let mut column = 0;
                for ch in state
                    .buffer()
                    .chunks_in(range.clone())?
                    .flat_map(str::chars)
                {
                    if column >= left + width {
                        break;
                    }
                    let cells = scalar_width(ch, column, *tab_width);
                    if column + cells > left {
                        emit(
                            &scalar_text(ch, column, *tab_width),
                            column,
                            cells,
                            &(offset..offset + ch.len_utf8()),
                        );
                    }
                    column += cells;
                    offset += ch.len_utf8();
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CachedLine {
    row: usize,
    layout: Layout,
}

/// Viewport and bounded layout cache owned exclusively by the terminal frontend.
#[derive(Debug, Default)]
pub struct Viewport {
    /// First logical document line displayed.
    pub top: usize,
    /// Horizontal scroll offset in terminal cells.
    pub left: usize,
    revision: Option<u64>,
    cache: VecDeque<CachedLine>,
    cache_bytes: usize,
    tab_width: usize,
    highlighter: Highlighter,
}
impl Viewport {
    /// Document byte under a text-area cell (`row` from the top of the view,
    /// `column` after the gutter), for mouse clicks. Rows below the document
    /// select its last line.
    pub fn byte_at_cell(
        &mut self,
        state: &EditorState,
        row: usize,
        column: usize,
    ) -> Result<usize, FrontendError> {
        let line = (self.top + row).min(state.line_count().saturating_sub(1));
        let column = self.left + column;
        Ok(self.line(state, line)?.byte_at(column))
    }
    /// Syntax language detected for the document shown in this viewport.
    pub fn language(&self) -> embedide_editor::syntax::Language {
        self.highlighter.language()
    }
    /// Configure tab rendering and invalidate cached line layouts.
    pub fn set_tab_width(&mut self, width: usize) {
        let width = width.clamp(1, 16);
        if self.tab_width != width {
            self.tab_width = width;
            self.revision = None;
        }
    }
    fn line(&mut self, state: &EditorState, row: usize) -> Result<&Layout, FrontendError> {
        if self.revision != Some(state.buffer().revision()) {
            self.cache.clear();
            self.cache_bytes = 0;
            self.revision = Some(state.buffer().revision());
        }
        let index = match self.cache.iter().position(|entry| entry.row == row) {
            Some(index) => index,
            None => {
                let range = state
                    .line_range(row)
                    .ok_or(embedide_editor::EditError::InvalidRange)?;
                let layout = if range.len() <= UNICODE_LINE_LIMIT {
                    Layout::from_text(
                        &state.buffer().slice(range.clone())?,
                        range.start,
                        if self.tab_width == 0 {
                            TAB_STOP
                        } else {
                            self.tab_width
                        },
                    )
                } else {
                    Layout::Escaped {
                        range,
                        tab_width: if self.tab_width == 0 {
                            TAB_STOP
                        } else {
                            self.tab_width
                        },
                    }
                };
                let bytes = layout.memory();
                while self.cache_bytes + bytes > CACHE_BYTES && !self.cache.is_empty() {
                    if let Some(entry) = self.cache.pop_front() {
                        self.cache_bytes -= entry.layout.memory();
                    }
                }
                self.cache_bytes += bytes;
                self.cache.push_back(CachedLine { row, layout });
                self.cache.len() - 1
            }
        };
        Ok(&self.cache[index].layout)
    }
}

/// A complete, bounded frame; the screen adapter emits only changed rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Rendered rows, capped to 256; usable width is capped to 1023 cells.
    pub rows: Vec<Row>,
    /// Visible terminal cursor (column, row), or None for a tiny viewport.
    pub cursor: Option<(u16, u16)>,
    /// Actual terminal dimensions, used to invalidate screen state on resize.
    pub size: (u16, u16),
    /// Insert cursor shape rather than the Normal block cursor.
    pub insert: bool,
}

pub(crate) fn label(text: &str, width: usize, reverse: bool) -> Row {
    let layout = Layout::from_text(text, 0, TAB_STOP);
    let mut row = Row::default();
    if let Layout::Unicode { tokens, .. } = layout {
        for token in tokens {
            if token.column + token.width > width {
                break;
            }
            row.push(&token.text, reverse);
        }
    }
    row
}

/// Compose a frame using cached grapheme layouts and terminal-cell scrolling.
/// Long lines (>16 KiB) use bounded-memory escaped scalar rendering.
pub fn compose(
    editor: &Editor,
    message: &str,
    view: &mut Viewport,
    size: (u16, u16),
) -> Result<Frame, FrontendError> {
    let width = usize::from(size.0.min(1024)).saturating_sub(1);
    let height = usize::from(size.1.min(256));
    let body = height.saturating_sub(2);
    let state = editor.state();
    let pos = state.position()?;
    let digits = state.line_count().to_string().len();
    let gutter = if width > digits + 3 { digits + 2 } else { 0 };
    let text_width = width.saturating_sub(gutter);
    let (column, cursor_width) = view.line(state, pos.line)?.cursor(state, state.cursor())?;
    if pos.line < view.top {
        view.top = pos.line;
    } else if pos.line >= view.top.saturating_add(body) {
        view.top = pos.line.saturating_sub(body.saturating_sub(1));
    }
    if column < view.left || cursor_width > text_width {
        view.left = column;
    } else if column + cursor_width > view.left.saturating_add(text_width) {
        view.left = (column + cursor_width).saturating_sub(text_width);
    }
    let mut rows = Vec::with_capacity(height);
    let left = view.left;
    for screen_row in 0..body {
        let row = view.top + screen_row;
        let mut painted = Row::default();
        if row < state.line_count() {
            if gutter > 0 {
                painted.push(&format!(" {:>digits$} ", row + 1), false);
            }
            let spans = view.highlighter.spans(state, row);
            view.line(state, row)?.paint(
                state,
                left,
                text_width,
                state.selection(),
                &spans,
                &mut painted,
            )?;
        } else if width > 0 {
            painted.push("~", false);
        }
        rows.push(painted);
    }
    if height > 1 {
        let name = state
            .path()
            .and_then(std::path::Path::file_name)
            .map(|name| name.to_string_lossy())
            .unwrap_or("[untitled]".into());
        let context = format!(
            "{:?} {}{}  {}:{}",
            editor.input().mode(),
            name,
            if state.is_modified() { " *" } else { "" },
            pos.line + 1,
            pos.scalar_column + 1
        );
        let status = if message.is_empty() {
            context
        } else {
            format!("{message} | {context}")
        };
        rows.push(label(&status, width, true));
    }
    let mut cursor = if body > 0 && text_width > 0 {
        Some((
            (gutter + column.saturating_sub(left)).min(width.saturating_sub(1)) as u16,
            (pos.line - view.top) as u16,
        ))
    } else {
        None
    };
    if height > 0 {
        if let Some(prompt) = editor.input().command_prompt() {
            let text = format!("{prompt}{}", editor.input().command_text().unwrap_or(""));
            let layout = Layout::from_text(&text, 0, TAB_STOP);
            let (end, _) = layout.cursor(state, text.len())?;
            let scroll = end.saturating_sub(width.saturating_sub(1));
            let mut row = Row::default();
            layout.paint(state, scroll, width, None, &[], &mut row)?;
            rows.push(row);
            cursor =
                (width > 0).then_some((end.saturating_sub(scroll) as u16, (height - 1) as u16));
        } else {
            rows.push(label(
                if message.is_empty() {
                    "i: insert   /: search   :w save   :q quit"
                } else {
                    message
                },
                width,
                false,
            ));
        }
    }
    Ok(Frame {
        rows,
        cursor,
        size,
        insert: editor.input().mode() == embedide_editor::vim::Mode::Insert,
    })
}
