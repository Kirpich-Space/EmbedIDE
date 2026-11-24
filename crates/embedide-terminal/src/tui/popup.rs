//! Completion popup: candidate list state and its cell-exact drawing.
//!
//! Selection rules (documented in `:help`): automatic popups start with
//! nothing selected, so Enter and Tab keep their normal meaning until
//! Down/Ctrl+N (or Up/Ctrl+P) selects an item; a manual popup (Ctrl+Space)
//! selects the first item. Tab or Enter accept the selected item, Esc
//! closes the popup and stays in Insert mode.
use super::layout::{self, Row};
use embedide_editor::completion::{rank, Candidate};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Items listed at once.
pub const VISIBLE: usize = 8;
/// Candidates ranked per keystroke.
const RANKED: usize = 200;

/// An open completion session in one document.
#[derive(Debug)]
pub struct Popup {
    /// Document index in the workspace.
    pub doc: usize,
    /// Byte offset where the completed word starts.
    pub start: usize,
    /// Candidates of the current source.
    pub items: Vec<Candidate>,
    /// `words` or the language server's name.
    pub source: String,
    /// Items come from a language server.
    pub semantic: bool,
    /// Ranked indices into `items`.
    pub shown: Vec<usize>,
    /// Selected position in `shown`.
    pub selected: Option<usize>,
    /// Outstanding language-server request.
    pub request: Option<u64>,
    /// The server asked to be re-queried as the user types.
    pub incomplete: bool,
    /// Key of the language server asked for this session.
    pub server: Option<String>,
    /// Opened explicitly via Ctrl+Space (keeps first item selected across LSP updates).
    pub manual: bool,
    query: String,
}

impl Popup {
    /// Open with `items` filtered by `query`; `manual` preselects the first item.
    pub fn new(
        doc: usize,
        start: usize,
        items: Vec<Candidate>,
        source: &str,
        query: &str,
        manual: bool,
    ) -> Self {
        let mut popup = Self {
            doc,
            start,
            items,
            source: source.into(),
            semantic: false,
            shown: Vec::new(),
            selected: None,
            request: None,
            incomplete: false,
            server: None,
            manual,
            query: String::new(),
        };
        popup.refilter(query);
        if manual && !popup.shown.is_empty() {
            popup.selected = Some(0);
        }
        popup
    }

    /// Re-rank for what has been typed; the selected item stays selected
    /// while it still matches.
    pub fn refilter(&mut self, query: &str) {
        let chosen = self.chosen().map(|c| c.label.clone());
        self.query = query.into();
        self.shown = rank(&self.items, query, RANKED);
        self.selected = chosen.and_then(|label| {
            self.shown
                .iter()
                .position(|&i| self.items[i].label == label)
        });
    }

    /// Replace the candidates (e.g. when the language server answers).
    pub fn replace(&mut self, items: Vec<Candidate>, source: &str, semantic: bool) {
        let keep_selection = self.selected.is_some() || self.manual;
        self.items = items;
        self.source = source.into();
        self.semantic = semantic;
        let query = std::mem::take(&mut self.query);
        self.refilter(&query);
        if keep_selection && self.selected.is_none() && !self.shown.is_empty() {
            self.selected = Some(0);
        }
    }

    /// The text typed since the word start.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Nothing to show.
    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// Select the next item (wrapping).
    pub fn next(&mut self) {
        if !self.shown.is_empty() {
            self.selected = Some(self.selected.map_or(0, |s| (s + 1) % self.shown.len()));
        }
    }

    /// Select the previous item (wrapping).
    pub fn previous(&mut self) {
        if !self.shown.is_empty() {
            let last = self.shown.len() - 1;
            self.selected = Some(
                self.selected
                    .map_or(last, |s| s.checked_sub(1).unwrap_or(last)),
            );
        }
    }

    /// The selected candidate.
    pub fn chosen(&self) -> Option<&Candidate> {
        self.selected
            .and_then(|s| self.shown.get(s))
            .map(|&i| &self.items[i])
    }

    /// Popup lines (items, then a source line) no wider than `max_width`,
    /// at most `max_rows` lines; returns them and their common width.
    pub fn rows(&self, max_rows: usize, max_width: usize) -> (Vec<Row>, usize) {
        let items_rows = max_rows.saturating_sub(1).min(VISIBLE);
        if items_rows == 0 || max_width < 8 {
            return (Vec::new(), 0);
        }
        let first = match self.selected {
            Some(s) if s >= items_rows => s + 1 - items_rows,
            _ => 0,
        };
        let page: Vec<&Candidate> = self
            .shown
            .iter()
            .skip(first)
            .take(items_rows)
            .map(|&i| &self.items[i])
            .collect();
        let label_w = page
            .iter()
            .map(|c| c.label.width())
            .max()
            .unwrap_or(0)
            .min(32);
        let kind_w = page
            .iter()
            .map(|c| c.kind.width())
            .max()
            .unwrap_or(0)
            .min(10);
        let detail_w = page
            .iter()
            .map(|c| c.detail.width())
            .max()
            .unwrap_or(0)
            .min(36);
        let tag = if self.semantic {
            self.source.clone()
        } else if self.source == "asm" {
            "asm: labels, mnemonics, registers (local)".into()
        } else if self.source == "project" {
            "project + lang (local)".into()
        } else {
            "words (local, no types)".into()
        };
        let footer = format!(" {tag} · {} · ↑↓ Tab/Enter Esc", self.shown.len());
        let mut width = 2 + label_w + 1 + kind_w + if detail_w > 0 { 1 + detail_w } else { 0 } + 1;
        // The source tag always fits: local words must never look semantic.
        width = width.max(24).max(tag.width() + 2).min(max_width);
        let mut rows: Vec<Row> = page
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let selected = self.selected == Some(first + i);
                let line = format!(
                    "{}{} {} {}",
                    if selected { ">" } else { " " },
                    fit(&c.label, label_w + 1),
                    fit(&c.kind, kind_w),
                    c.detail
                );
                // Selected item: normal video in an inverse popup.
                layout::label(&fit(&line, width), width, !selected)
            })
            .collect();
        rows.push(layout::label(&fit(&footer, width), width, true));
        (rows, width)
    }
}

/// `text` cut to `width` terminal cells and padded with spaces to exactly
/// `width` (wide characters never split; control characters dropped).
pub fn fit(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        if ch.is_control() {
            continue;
        }
        let w = ch.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push_str(&" ".repeat(width - used));
    out
}

/// `row` with the cells `col..col + width` replaced by `piece`. A wide
/// character cut by either edge becomes spaces, so columns never shift.
pub fn overlay(row: &Row, col: usize, piece: &Row, width: usize) -> Row {
    let end = col + width;
    let mut out = Row::default();
    let mut inserted = false;
    let mut pos = 0; // cell position in `row`
    let insert = |out: &mut Row| {
        for run in &piece.runs {
            out.push_attrs(&run.text, run.selected, run.style, run.cell);
        }
    };
    for run in &row.runs {
        let mut text = String::new();
        for ch in run.text.chars() {
            let (from, to) = (pos, pos + ch.width().unwrap_or(0));
            pos = to;
            if to <= col || (inserted && from >= end) {
                text.push(ch);
                continue;
            }
            if !inserted {
                text.push_str(&" ".repeat(col.saturating_sub(from)));
                out.push_attrs(
                    &std::mem::take(&mut text),
                    run.selected,
                    run.style,
                    run.cell,
                );
                insert(&mut out);
                inserted = true;
            }
            if to > end {
                text.push_str(&" ".repeat(to - end.max(from)));
            }
        }
        out.push_attrs(&text, run.selected, run.style, run.cell);
    }
    if !inserted {
        out.push(&" ".repeat(col.saturating_sub(pos)), false);
        insert(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(row: &Row) -> String {
        row.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn overlay_keeps_columns_when_it_cuts_wide_characters() {
        let mut row = Row::default();
        row.push("ab中文cd", false);
        let mut piece = Row::default();
        piece.push("XYZ", true);
        // Cells: a0 b1 中2-3 文4-5 c6 d7. Piece over 3..6 cuts both CJK characters.
        let out = overlay(&row, 3, &piece, 3);
        assert_eq!(text(&out), "ab XYZcd");
        assert_eq!(UnicodeWidthStr::width(text(&out).as_str()), 8);
        assert!(out.runs.iter().any(|r| r.selected && r.text == "XYZ"));
        // Past the end of a short row: padded up to the column.
        let out = overlay(&row, 10, &piece, 3);
        assert_eq!(text(&out), "ab中文cd  XYZ");
    }

    #[test]
    fn fit_pads_and_truncates_by_cells_and_navigation_wraps() {
        assert_eq!(fit("中文x", 3), "中 ");
        assert_eq!(fit("ab", 4), "ab  ");
        let items = ["alpha", "alps", "beta"].map(Candidate::word).to_vec();
        let mut popup = Popup::new(0, 0, items, "words", "al", false);
        assert_eq!(popup.shown.len(), 2);
        assert!(popup.chosen().is_none());
        // Equal prefix matches: shorter first (alps, alpha).
        popup.previous();
        let label = |p: &Popup| p.chosen().unwrap().label.clone();
        assert_eq!(label(&popup), "alpha", "Up from nothing selects the last");
        popup.next();
        assert_eq!(label(&popup), "alps", "wraps");
        popup.refilter("alp");
        assert_eq!(label(&popup), "alps", "selection survives refiltering");
        let (rows, width) = popup.rows(10, 40);
        assert_eq!(rows.len(), 3, "two items and the source line");
        assert!(rows
            .iter()
            .all(|r| UnicodeWidthStr::width(text(r).as_str()) == width));
        assert!(text(&rows[2]).contains("words (local, no types)"));
    }
}
