//! The AI panel's input line (cursor movement, editing, history) and its
//! mouse selection. A drag inside the transcript selects only the panel's
//! own text (not the editor or explorer beside it) and copies it to the
//! terminal's clipboard (OSC 52) when the button is released.
use super::{layout::Row, Assistant, Line};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthChar;

/// Longest input accepted, in characters.
const MAX_INPUT: usize = 16384;
/// Remembered inputs.
const HISTORY: usize = 100;

/// A selection in transcript coordinates: (line, cell column).
#[derive(Debug, Clone, Copy)]
pub(super) struct Selection {
    anchor: (usize, usize),
    head: (usize, usize),
    dragging: bool,
}

impl Selection {
    fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

fn cw(c: char) -> usize {
    c.width().unwrap_or(1)
}

/// Split `text` at cell columns `from..=to` (inclusive end) into
/// (before, selected, after).
fn split_cells(text: &str, from: usize, to: usize) -> (String, String, String) {
    let (mut a, mut b, mut c) = (String::new(), String::new(), String::new());
    let mut col = 0;
    for ch in text.chars() {
        let w = cw(ch);
        let target = if col + w <= from {
            &mut a
        } else if col > to {
            &mut c
        } else {
            &mut b
        };
        target.push(ch);
        col += w;
    }
    (a, b, c)
}

fn line_text(line: &Line) -> String {
    line.iter().map(|(t, _)| t.as_str()).collect()
}

impl Assistant {
    /// Cursor position in the input, in characters.
    pub fn input_cursor(&self) -> usize {
        self.cursor.min(self.input.chars().count())
    }

    /// Replace the input and put the cursor at its end.
    pub fn set_input(&mut self, text: impl Into<String>) {
        self.input = text.into();
        self.cursor = self.input.chars().count();
        self.hist_pos = None;
    }

    /// Empty the input.
    pub fn clear_input(&mut self) {
        self.set_input("");
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.input
            .char_indices()
            .nth(chars)
            .map_or(self.input.len(), |(i, _)| i)
    }

    /// Insert typed or pasted text at the cursor (control characters are
    /// dropped, line breaks become spaces).
    pub fn insert_text(&mut self, text: &str) {
        let clean: String = text
            .chars()
            .map(|c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    ' '
                } else {
                    c
                }
            })
            .filter(|c| !c.is_control())
            .collect();
        let room = MAX_INPUT.saturating_sub(self.input.chars().count());
        let clean: String = clean.chars().take(room).collect();
        if clean.is_empty() {
            return;
        }
        let at = self.byte_at(self.input_cursor());
        self.input.insert_str(at, &clean);
        self.cursor = self.input_cursor() + clean.chars().count();
        self.hist_pos = None;
    }

    fn word_left(&self) -> usize {
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = self.input_cursor();
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = self.input_cursor();
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn delete_range(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte_at(from), self.byte_at(to));
        self.input.replace_range(a..b, "");
        self.cursor = from;
        self.hist_pos = None;
    }

    /// Remember a submitted input for Up/Down recall.
    pub(super) fn remember_input(&mut self, line: &str) {
        if self.history_inputs.last().map(String::as_str) != Some(line) {
            self.history_inputs.push(line.to_string());
            if self.history_inputs.len() > HISTORY {
                self.history_inputs.remove(0);
            }
        }
        self.hist_pos = None;
    }

    fn recall(&mut self, older: bool) {
        let len = self.history_inputs.len();
        if len == 0 {
            return;
        }
        let next = match (self.hist_pos, older) {
            (None, true) => {
                self.draft = self.input.clone();
                Some(len - 1)
            }
            (None, false) => return,
            (Some(0), true) => Some(0),
            (Some(i), true) => Some(i - 1),
            (Some(i), false) if i + 1 < len => Some(i + 1),
            (Some(_), false) => None,
        };
        self.input = match next {
            Some(i) => self.history_inputs[i].clone(),
            None => std::mem::take(&mut self.draft),
        };
        self.cursor = self.input.chars().count();
        self.hist_pos = next;
    }

    /// Handle an editing or movement key for the input line. Returns whether
    /// the key was used.
    ///
    /// Left/Right move (Ctrl or Alt: by word), Home/End or Ctrl+A/Ctrl+E jump,
    /// Backspace/Delete erase (Ctrl+Backspace/Ctrl+Delete: a word), Ctrl+U
    /// erases to the start, Up/Down recall earlier inputs, Shift+Up/Down
    /// scroll the transcript.
    pub fn edit_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let word = ctrl || alt;
        let cur = self.input_cursor();
        let len = self.input.chars().count();
        match key.code {
            KeyCode::Left if word => self.cursor = self.word_left(),
            KeyCode::Right if word => self.cursor = self.word_right(),
            KeyCode::Left => self.cursor = cur.saturating_sub(1),
            KeyCode::Right => self.cursor = (cur + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            KeyCode::Char('a') if ctrl && !alt => self.cursor = 0,
            KeyCode::Char('e') if ctrl && !alt => self.cursor = len,
            KeyCode::Char('b') if alt && !ctrl => self.cursor = self.word_left(),
            KeyCode::Char('f') if alt && !ctrl => self.cursor = self.word_right(),
            KeyCode::Char('u') if ctrl && !alt => self.delete_range(0, cur),
            KeyCode::Backspace if word => {
                let to = self.word_left();
                self.delete_range(to, cur);
            }
            KeyCode::Backspace => {
                if cur > 0 {
                    self.delete_range(cur - 1, cur);
                }
            }
            KeyCode::Delete if word => {
                let to = self.word_right();
                self.delete_range(cur, to);
            }
            KeyCode::Delete => {
                if cur < len {
                    self.delete_range(cur, cur + 1);
                }
            }
            KeyCode::Up if shift => self.scroll(1),
            KeyCode::Down if shift => self.scroll(-1),
            KeyCode::Up if !word => self.recall(true),
            KeyCode::Down if !word => self.recall(false),
            KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => {
                self.insert_text(ch.encode_utf8(&mut [0; 4]));
            }
            _ => return false,
        }
        true
    }

    /// The part of the input shown in a panel `width` cells wide:
    /// (first char, one past the last char, cursor char), scrolled so the
    /// cursor is visible.
    pub(super) fn input_window(&self, width: usize) -> (usize, usize, usize) {
        let chars: Vec<char> = self.input.chars().collect();
        let cur = self.input_cursor();
        let avail = width.saturating_sub(3);
        let mut start = 0;
        let mut before: usize = chars[..cur].iter().map(|c| cw(*c)).sum();
        while before > avail && start < cur {
            before -= cw(chars[start]);
            start += 1;
        }
        let mut used = 0;
        let mut end = start;
        while end < chars.len() {
            let w = cw(chars[end]);
            if used + w > avail {
                break;
            }
            used += w;
            end += 1;
        }
        (start, end, cur)
    }

    /// Where the transcript starts in a panel `height` rows high.
    fn view_start(&self, lines: usize, height: usize) -> usize {
        let body_h = height.saturating_sub(2);
        let end = lines.saturating_sub(self.scroll.min(lines));
        end.saturating_sub(body_h)
    }

    /// Whether a mouse selection is in progress.
    pub fn selecting(&self) -> bool {
        self.sel.is_some_and(|s| s.dragging)
    }

    /// Whether any text is selected.
    pub fn has_selection(&self) -> bool {
        self.sel.is_some_and(|s| s.anchor != s.head)
    }

    /// Forget the selection.
    pub fn clear_selection(&mut self) {
        self.sel = None;
    }

    fn cell_at(&self, x: usize, y: usize, width: usize, height: usize) -> (usize, usize) {
        let lines = self.transcript_lines(width);
        let start = self.view_start(lines.len(), height);
        let row = y.saturating_sub(1).min(height.saturating_sub(3));
        let line = (start + row).min(lines.len().saturating_sub(1));
        (line, x.min(width.saturating_sub(1)))
    }

    /// Button pressed at panel-relative (`x`, `y`). Returns whether the
    /// press was in the transcript (a selection started).
    pub fn mouse_down(&mut self, x: usize, y: usize, width: usize, height: usize) -> bool {
        self.sel = None;
        if height < 3 || y == 0 {
            return false;
        }
        if y + 1 >= height {
            // The input line: put the cursor where the click was.
            let (start, end, _) = self.input_window(width);
            let target = x.saturating_sub(2);
            let chars: Vec<char> = self.input.chars().collect();
            let mut col = 0;
            let mut at = start;
            while at < end && col + cw(chars[at]) <= target {
                col += cw(chars[at]);
                at += 1;
            }
            self.cursor = at;
            return false;
        }
        let at = self.cell_at(x, y, width, height);
        self.sel = Some(Selection {
            anchor: at,
            head: at,
            dragging: true,
        });
        true
    }

    /// Button dragged to panel-relative (`x`, `y`) (may be outside the panel).
    pub fn mouse_drag(&mut self, x: isize, y: isize, width: usize, height: usize) {
        let Some(mut sel) = self.sel.filter(|s| s.dragging) else {
            return;
        };
        // Dragging past the top or bottom edge scrolls the transcript.
        if y < 1 {
            self.scroll(1);
        } else if y as usize + 2 > height {
            self.scroll(-1);
        }
        let x = x.max(0) as usize;
        let y = y.max(1) as usize;
        sel.head = self.cell_at(x, y.min(height.saturating_sub(2)), width, height);
        self.sel = Some(sel);
    }

    /// Button released: the selected text to copy, if any.
    pub fn mouse_up(&mut self, width: usize) -> Option<String> {
        let mut sel = self.sel?;
        sel.dragging = false;
        if sel.anchor == sel.head {
            self.sel = None;
            return None;
        }
        self.sel = Some(sel);
        let text = self.selected_text(width);
        (!text.is_empty()).then_some(text)
    }

    /// The selected text: decoration (indent, `│`, `✎`, …) is left out of
    /// whole lines, trailing spaces are trimmed.
    pub fn selected_text(&self, width: usize) -> String {
        let Some(sel) = self.sel else {
            return String::new();
        };
        let lines = self.transcript_lines(width);
        let ((l0, c0), (l1, c1)) = sel.ordered();
        let mut out: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate().take(l1 + 1).skip(l0) {
            let from = if i == l0 { c0 } else { 0 };
            let to = if i == l1 { c1 } else { usize::MAX };
            let text = if from == 0 && line.len() > 1 {
                // A whole-line start: drop the decorative lead.
                let lead = &line[0].0;
                if lead.chars().all(|c| " │·✗✓✎⊘⚠+▍".contains(c)) {
                    let rest: String = line[1..].iter().map(|(t, _)| t.as_str()).collect();
                    let skip = lead.chars().map(cw).sum::<usize>();
                    let (_, mid, _) = split_cells(&rest, 0, to.saturating_sub(skip));
                    mid
                } else {
                    split_cells(&line_text(line), from, to).1
                }
            } else {
                split_cells(&line_text(line), from, to).1
            };
            out.push(text.trim_end().to_string());
        }
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out.join("\n")
    }

    /// One transcript row with the selected cells highlighted.
    pub(super) fn selected_row(&self, index: usize, line: &Line) -> Row {
        let mut row = Row::default();
        let sel = self.sel.map(|s| s.ordered());
        let range = sel.and_then(|((l0, c0), (l1, c1))| {
            (l0..=l1).contains(&index).then_some({
                (
                    if index == l0 { c0 } else { 0 },
                    if index == l1 { c1 } else { usize::MAX },
                )
            })
        });
        let Some((from, to)) = range else {
            for (text, style) in line {
                row.push_styled(text, false, *style);
            }
            return row;
        };
        let mut col = 0;
        for (text, style) in line {
            let width: usize = text.chars().map(cw).sum();
            if col + width <= from || col > to {
                row.push_styled(text, false, *style);
            } else {
                let (a, b, c) = split_cells(text, from.saturating_sub(col), to.saturating_sub(col));
                for (part, selected) in [(a, false), (b, true), (c, false)] {
                    if !part.is_empty() {
                        row.push_styled(&part, selected, *style);
                    }
                }
            }
            col += width;
        }
        row
    }
}

/// Standard base64 (for OSC 52).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The OSC 52 sequence that copies `text` to the terminal's clipboard.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedide_workspace::config::AiConfig;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn press(a: &mut Assistant, code: KeyCode) {
        assert!(a.edit_key(key(code, KeyModifiers::NONE)), "{code:?}");
    }

    #[test]
    fn arrows_home_end_delete_and_words_edit_the_middle_of_the_input() {
        let mut a = Assistant::new(AiConfig::default());
        for ch in "fix the bug".chars() {
            press(&mut a, KeyCode::Char(ch));
        }
        assert_eq!(a.input, "fix the bug");
        for _ in 0..3 {
            press(&mut a, KeyCode::Left);
        }
        press(&mut a, KeyCode::Char('!'));
        assert_eq!(a.input, "fix the !bug");
        press(&mut a, KeyCode::Backspace);
        press(&mut a, KeyCode::Delete);
        assert_eq!(a.input, "fix the ug");
        press(&mut a, KeyCode::Home);
        press(&mut a, KeyCode::Char('>'));
        assert_eq!(a.input, ">fix the ug");
        press(&mut a, KeyCode::End);
        assert!(a.edit_key(key(KeyCode::Left, KeyModifiers::CONTROL)));
        assert_eq!(a.input_cursor(), 9, "start of `ug`");
        assert!(a.edit_key(key(KeyCode::Backspace, KeyModifiers::CONTROL)));
        assert_eq!(a.input, ">fix ug");
        assert!(a.edit_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL)));
        assert_eq!(a.input, "ug");
        // Unicode and wide characters edit by character
        a.set_input("猫é");
        press(&mut a, KeyCode::Left);
        press(&mut a, KeyCode::Backspace);
        assert_eq!(a.input, "é");
        // Keys it does not own are not used
        assert!(!a.edit_key(key(KeyCode::F(5), KeyModifiers::NONE)));
        assert!(!a.edit_key(key(KeyCode::Char('k'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn up_and_down_recall_earlier_inputs_and_restore_the_draft() {
        let mut a = Assistant::new(AiConfig::default());
        for text in ["/status", "/help"] {
            a.remember_input(text);
        }
        a.set_input("draft");
        press(&mut a, KeyCode::Up);
        assert_eq!(a.input, "/help");
        press(&mut a, KeyCode::Up);
        assert_eq!(a.input, "/status");
        press(&mut a, KeyCode::Up);
        assert_eq!(a.input, "/status");
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Down);
        assert_eq!(a.input, "draft");
        // Editing a recalled input leaves history mode
        press(&mut a, KeyCode::Up);
        press(&mut a, KeyCode::Char('x'));
        assert_eq!(a.input, "/helpx");
        assert!(a.hist_pos.is_none());
    }

    #[test]
    fn paste_goes_in_at_the_cursor_as_one_line() {
        let mut a = Assistant::new(AiConfig::default());
        a.set_input("ab");
        press(&mut a, KeyCode::Left);
        a.insert_text("1\n2\x1b[31m3");
        assert_eq!(a.input, "a1 2[31m3b");
        assert_eq!(a.input_cursor(), 9);
    }

    #[test]
    fn a_long_input_scrolls_to_keep_the_cursor_visible() {
        let mut a = Assistant::new(AiConfig::default());
        a.set_input("x".repeat(50));
        let (start, end, cur) = a.input_window(20);
        assert_eq!((cur, end - start), (50, 17));
        assert!(start > 0);
        a.cursor = 0;
        assert_eq!(a.input_window(20), (0, 17, 0));
    }

    #[test]
    fn dragging_selects_only_the_panel_text_and_copies_it() {
        let mut a = Assistant::new(AiConfig::default());
        a.note(super::super::Kind::Header, "Codex CLI");
        a.note(super::super::Kind::Assistant, "alpha beta gamma delta");
        a.note(super::super::Kind::Info, "second note");
        let (w, h) = (30, 12);
        let (rows, _) = a.rows(w, h, true);
        let text = |r: &Row| r.runs.iter().map(|x| x.text.as_str()).collect::<String>();
        let row_of = |rows: &[Row], needle: &str| {
            rows.iter().position(|r| text(r).contains(needle)).unwrap()
        };
        let y = row_of(&rows, "alpha");
        assert!(a.mouse_down(4, y, w, h));
        a.mouse_drag(9, y as isize, w, h);
        let copied = a.mouse_up(w).expect("copied text");
        assert_eq!(copied, "pha be");
        let (rows, _) = a.rows(w, h, true);
        let selected: String = rows[y]
            .runs
            .iter()
            .filter(|r| r.selected)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(selected, "pha be", "only the dragged cells are highlighted");
        assert!(rows
            .iter()
            .enumerate()
            .skip(1)
            .all(|(i, r)| i == y || r.runs.iter().all(|x| !x.selected)));
        // Several whole lines: decoration is left out
        let y2 = row_of(&rows, "second");
        assert!(a.mouse_down(0, y, w, h));
        a.mouse_drag(29, y2 as isize, w, h);
        assert_eq!(
            a.mouse_up(w).unwrap(),
            "alpha beta gamma delta\nsecond note"
        );
        // A click without a drag selects nothing; clicking the input line moves its cursor
        a.set_input("hello");
        assert!(!a.mouse_down(5, y, w, h) || a.mouse_up(w).is_none());
        a.mouse_down(4, h - 1, w, h);
        assert_eq!(a.input_cursor(), 2);
        assert!(!a.has_selection());
    }

    #[test]
    fn osc52_encodes_base64() {
        assert_eq!(base64(b"hello"), "aGVsbG8=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64("猫".as_bytes()), "54yr");
        assert_eq!(osc52("a"), "\x1b]52;c;YQ==\x07");
    }
}
