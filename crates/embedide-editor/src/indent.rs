//! Language-aware automatic indentation, independent of any frontend.
//!
//! Rules look at code only: the syntax lexer blanks strings and comments
//! first, so a `{` inside `"…"` or `// …` never changes indentation, and
//! nothing happens while the cursor is inside a block comment or string.
//! Pasted text is inserted literally; only Enter, typed closers and the
//! explicit Tab/Shift+Tab operations re-indent.
use crate::{
    syntax::{highlight_line, Highlight, Language, LineState},
    EditorError, EditorState,
};
use std::path::Path;

/// How many lines above the cursor are lexed to find the lexer state and
/// the opener of a closing delimiter. Bounds the work per keystroke.
const LOOKBACK_LINES: usize = 2000;
/// Longest leading whitespace considered (pathological lines stay as they are).
const MAX_INDENT_BYTES: usize = 4096;

/// Indentation settings of one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentStyle {
    /// Columns per level (1..=16).
    pub width: usize,
    /// Indent with tab characters instead of spaces.
    pub tabs: bool,
    /// Language-aware indentation on Enter and for typed closers. When off,
    /// Enter keeps the current indentation only.
    pub auto: bool,
}

impl Default for IndentStyle {
    fn default() -> Self {
        Self {
            width: 4,
            tabs: false,
            auto: true,
        }
    }
}

impl IndentStyle {
    /// Settings with `width` clamped to 1..=16.
    pub fn new(width: usize, tabs: bool, auto: bool) -> Self {
        Self {
            width: width.clamp(1, 16),
            tabs,
            auto,
        }
    }

    /// Adopt the file's own convention: tab-indented files (and Makefiles,
    /// whose recipes require tabs) use tabs; space-indented files use spaces
    /// with the configured width. Files without indentation keep `self`.
    pub fn detect(self, path: Option<&Path>, text: &str) -> Self {
        let makefile = path.is_some_and(|p| {
            p.extension().is_some_and(|e| e == "mk")
                || p.file_name().is_some_and(|n| {
                    ["Makefile", "makefile", "GNUmakefile"]
                        .iter()
                        .any(|m| n == *m)
                })
        });
        if makefile {
            return Self { tabs: true, ..self };
        }
        let (mut tabs, mut spaces) = (0usize, 0usize);
        for line in text.lines().take(2000) {
            match line.as_bytes().first() {
                Some(b'\t') => tabs += 1,
                Some(b' ') if line.trim_start().len() < line.len() => spaces += 1,
                _ => {}
            }
        }
        if tabs > spaces {
            Self { tabs: true, ..self }
        } else if spaces > tabs {
            Self {
                tabs: false,
                ..self
            }
        } else {
            self
        }
    }

    /// Display columns of leading whitespace (a tab advances to the next level).
    pub fn columns(&self, whitespace: &str) -> usize {
        whitespace.chars().fold(0, |col, ch| {
            if ch == '\t' {
                (col / self.width + 1) * self.width
            } else {
                col + 1
            }
        })
    }

    /// Whitespace that reaches `columns` in this style.
    pub fn make(&self, columns: usize) -> String {
        if self.tabs {
            let mut s = "\t".repeat(columns / self.width);
            s.push_str(&" ".repeat(columns % self.width));
            s
        } else {
            " ".repeat(columns)
        }
    }
}

/// Indentation behaviour of a language family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rules {
    /// `{ [ (` open a level, `} ] )` close it (C, C++, Rust, Zig, JS/TS, JSON).
    Braces,
    /// A trailing `:` opens a level.
    Python,
    /// `{ (`, `then`, `do`, `else` open a level; `}` closes.
    Shell,
    /// A label line at column 0 indents the following instruction.
    Asm,
    /// Keep the current indentation (Fortran, TOML, text).
    Plain,
}

fn rules(language: Language) -> Rules {
    match language {
        Language::C
        | Language::Cpp
        | Language::Rust
        | Language::Zig
        | Language::JavaScript
        | Language::TypeScript
        | Language::Json => Rules::Braces,
        Language::Python => Rules::Python,
        Language::Shell => Rules::Shell,
        Language::Asm => Rules::Asm,
        Language::Fortran | Language::Toml | Language::Plain => Rules::Plain,
    }
}

/// Language of a document, as the highlighter detects it.
pub(crate) fn language(state: &EditorState) -> Language {
    let first = state
        .line_range(0)
        .and_then(|r| {
            let mut end = r.end.min(r.start + 256);
            while end > r.start && state.buffer.char_at(end).is_err() {
                end -= 1;
            }
            state.buffer.slice(r.start..end).ok()
        })
        .unwrap_or_default();
    Language::detect(state.path(), &first)
}

fn line_text(state: &EditorState, row: usize) -> Result<String, EditorError> {
    let start = state.lines.start(row);
    let end = state.lines.end(row, &state.buffer);
    Ok(state.buffer.slice(start..end)?)
}

/// The line with strings and comments replaced by spaces (same byte
/// length), plus the lexer state at its end.
fn code(language: Language, text: &str, start: &LineState) -> (String, LineState) {
    let mut spans = Vec::new();
    let end = highlight_line(language, text, start, &mut spans);
    let mut bytes = text.as_bytes().to_vec();
    for span in spans {
        if matches!(span.kind, Highlight::String | Highlight::Comment) {
            for b in &mut bytes[span.range.start.min(text.len())..span.range.end.min(text.len())] {
                *b = b' ';
            }
        }
    }
    // Only whole UTF-8 sequences were replaced, so the result stays valid;
    // fall back to the original text if a span ever split a character.
    (
        String::from_utf8(bytes).unwrap_or_else(|_| text.into()),
        end,
    )
}

/// Lexer state at the start of `row`, lexing at most `LOOKBACK_LINES` above.
fn state_at(state: &EditorState, language: Language, row: usize) -> Result<LineState, EditorError> {
    let mut current = LineState::Normal;
    for r in row.saturating_sub(LOOKBACK_LINES)..row {
        current = code(language, &line_text(state, r)?, &current).1;
    }
    Ok(current)
}

fn leading(text: &str) -> &str {
    let end = text
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(text.len())
        .min(MAX_INDENT_BYTES);
    &text[..end]
}

fn opens_level(rules: Rules, code: &str) -> bool {
    let code = code.trim_end();
    match rules {
        Rules::Braces => code.ends_with(['{', '[', '(']),
        Rules::Python => code.ends_with(':'),
        Rules::Shell => {
            code.ends_with(['{', '('])
                || code
                    .rsplit(|c: char| c.is_whitespace() || c == ';')
                    .next()
                    .is_some_and(|w| matches!(w, "then" | "do" | "else"))
        }
        Rules::Asm => {
            let word = code.trim();
            leading(code).is_empty()
                && word.ends_with(':')
                && !word[..word.len() - 1].contains(char::is_whitespace)
        }
        Rules::Plain => false,
    }
}

fn closer_for(open: u8) -> u8 {
    match open {
        b'{' => b'}',
        b'[' => b']',
        _ => b')',
    }
}

/// Text inserted by Enter at the cursor and the cursor offset within it.
pub(crate) fn newline(
    state: &EditorState,
) -> Result<(std::ops::Range<usize>, String, usize), EditorError> {
    let style = state.indent;
    let row = state.lines.row(state.cursor);
    let start = state.lines.start(row);
    let end = state.lines.end(row, &state.buffer);
    let before = state.buffer.slice(start..state.cursor)?;
    let after = state.buffer.slice(state.cursor..end)?;
    let base = leading(&before).to_string();
    let eol = state.eol;
    // Enter on a whitespace-only line leaves no trailing blanks behind.
    let range = if before.trim().is_empty() && after.trim().is_empty() && !before.is_empty() {
        start..state.cursor
    } else {
        state.cursor..state.cursor
    };
    let plain = |indent: &str| {
        let text = format!("{eol}{indent}");
        let cursor = text.len();
        Ok((range.clone(), text, cursor))
    };
    if !style.auto {
        return plain(&base);
    }
    let language = language(state);
    let rules = rules(language);
    let begin = state_at(state, language, row)?;
    let (code_before, inside) = code(language, &before, &begin);
    if inside != LineState::Normal || rules == Rules::Plain {
        return plain(&base);
    }
    if !opens_level(rules, &code_before) {
        return plain(&base);
    }
    let deeper = style.make(style.columns(&base) + style.width);
    let opener = code_before.trim_end().as_bytes().last().copied();
    let closes_here = rules == Rules::Braces
        && opener.is_some_and(|o| after.trim_start().as_bytes().first() == Some(&closer_for(o)));
    if closes_here {
        // `{|}` → `{`, an indented empty line for the cursor, `}` at the base.
        let first = format!("{eol}{deeper}");
        let cursor = first.len();
        let text = format!("{first}{eol}{base}");
        // Whitespace between the cursor and the closer is dropped.
        let gap = after.len() - after.trim_start().len();
        return Ok((state.cursor..state.cursor + gap, text, cursor));
    }
    plain(&deeper)
}

/// Re-indent the current line before a typed closer when it is the first
/// thing on the line: it aligns with the line that holds its opener.
/// Returns the replacement for the line's leading whitespace, if any.
pub(crate) fn closer(
    state: &EditorState,
    typed: char,
) -> Result<Option<(std::ops::Range<usize>, String)>, EditorError> {
    let style = state.indent;
    if !style.auto {
        return Ok(None);
    }
    let language = language(state);
    let rules = rules(language);
    let allowed = match rules {
        Rules::Braces => matches!(typed, '}' | ']' | ')'),
        Rules::Shell => typed == '}',
        _ => false,
    };
    if !allowed {
        return Ok(None);
    }
    let row = state.lines.row(state.cursor);
    let start = state.lines.start(row);
    let before = state.buffer.slice(start..state.cursor)?;
    if !before.trim().is_empty() {
        return Ok(None);
    }
    let open = match typed {
        '}' => b'{',
        ']' => b'[',
        _ => b'(',
    };
    let first = row.saturating_sub(LOOKBACK_LINES);
    let mut lexer = LineState::Normal;
    let mut codes = Vec::with_capacity(row - first);
    for r in first..row {
        let (code, end) = code(language, &line_text(state, r)?, &lexer);
        codes.push(code);
        lexer = end;
    }
    if lexer != LineState::Normal {
        return Ok(None); // inside a block comment or string
    }
    let mut depth = 0usize;
    for line in codes.iter().rev() {
        for &b in line.as_bytes().iter().rev() {
            if b == closer_for(open) {
                depth += 1;
            } else if b == open {
                if depth == 0 {
                    let indent = leading(line).to_string();
                    let current = leading(&before);
                    if indent == current {
                        return Ok(None);
                    }
                    return Ok(Some((start..start + before.len(), indent)));
                }
                depth -= 1;
            }
        }
    }
    Ok(None)
}

/// New leading whitespace for one more (or one less) level.
pub(crate) fn shifted(style: &IndentStyle, whitespace: &str, dedent: bool) -> String {
    let columns = style.columns(whitespace);
    let target = if dedent {
        columns.saturating_sub(1) / style.width * style.width
    } else {
        (columns / style.width + 1) * style.width
    };
    if dedent && columns == 0 {
        return String::new();
    }
    style.make(target)
}

/// Leading-whitespace range of `row` and its text.
pub(crate) fn line_indent(
    state: &EditorState,
    row: usize,
) -> Result<(std::ops::Range<usize>, String, bool), EditorError> {
    let text = line_text(state, row)?;
    let ws = leading(&text).to_string();
    let start = state.lines.start(row);
    let blank = text.trim().is_empty();
    Ok((start..start + ws.len(), ws, blank))
}

/// Whitespace inserted by Tab after the first non-blank character: a tab,
/// or spaces up to the next multiple of the indent width.
pub(crate) fn tab_text(state: &EditorState) -> Result<String, EditorError> {
    let style = state.indent;
    if style.tabs {
        return Ok("\t".into());
    }
    let row = state.lines.row(state.cursor);
    let before = state.buffer.slice(state.lines.start(row)..state.cursor)?;
    // Terminal cells, so tab stops line up after wide characters (中 = 2).
    let column = before.chars().fold(0, |col, ch| {
        if ch == '\t' {
            (col / style.width + 1) * style.width
        } else {
            col + unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
        }
    });
    Ok(" ".repeat(style.width - column % style.width))
}
