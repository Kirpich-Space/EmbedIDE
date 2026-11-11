//! Frontend-neutral completion data: the identifier at the cursor, local
//! word candidates and ranking. Semantic candidates come from a language
//! server (see `embedide_core::lsp`) and use the same [`Candidate`] type.
use crate::{EditorError, EditorState};
use std::{collections::BTreeSet, ops::Range};

/// Scanned for local words; larger documents use their first part only.
const WORD_SCAN_BYTES: usize = 4 * 1024 * 1024;
/// Distinct local words kept.
const MAX_WORDS: usize = 5000;
/// Longest local word offered.
const MAX_WORD_CHARS: usize = 64;

/// One suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Text shown in the list.
    pub label: String,
    /// Text inserted on acceptance (snippet syntax already removed).
    pub insert: String,
    /// Symbol kind, e.g. `function`, `struct`, or `word` for local words.
    pub kind: String,
    /// Short extra information (type, signature), if any.
    pub detail: String,
    /// Text matched against what was typed.
    pub filter: String,
    /// Server ranking key; ties keep server order.
    pub sort: String,
    /// Document byte range the server asked to replace, if it gave one.
    pub range: Option<Range<usize>>,
}

impl Candidate {
    /// A local word from the document.
    pub fn word(word: &str) -> Self {
        Self {
            label: word.into(),
            insert: word.into(),
            kind: "word".into(),
            detail: String::new(),
            filter: word.into(),
            sort: String::new(),
            range: None,
        }
    }
}

/// Characters that belong to identifiers (Unicode letters and digits, `_`).
pub fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Byte range of the identifier fragment that ends at the cursor (empty
/// when the cursor does not follow an identifier character).
pub fn word_before_cursor(state: &EditorState) -> Result<Range<usize>, EditorError> {
    let cursor = state.cursor();
    let row = state.lines.row(cursor);
    let start = state.lines.start(row);
    let before = state.buffer.slice(start..cursor)?;
    let begin = before
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_word_char(*ch))
        .last()
        .map_or(before.len(), |(i, _)| i);
    Ok(start + begin..cursor)
}

/// Distinct identifiers of the document (at least two characters, not
/// starting with a digit), except the occurrence the cursor is typing.
pub fn document_words(state: &EditorState) -> Vec<String> {
    let buffer = state.buffer();
    let mut end = buffer.byte_len().min(WORD_SCAN_BYTES);
    while end > 0 && buffer.char_at(end).is_err() {
        end -= 1;
    }
    let Ok(text) = buffer.slice(0..end) else {
        return Vec::new();
    };
    let typing = word_before_cursor(state).ok();
    let mut words = BTreeSet::new();
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if is_word_char(ch) {
            start.get_or_insert(index);
            continue;
        }
        let Some(begin) = start.take() else { continue };
        let word = &text[begin..index];
        let at_cursor = typing
            .as_ref()
            .is_some_and(|t| !t.is_empty() && t.start == begin && t.end <= index);
        if at_cursor
            || word.chars().next().is_some_and(|c| c.is_ascii_digit())
            || word.chars().count() < 2
            || word.chars().count() > MAX_WORD_CHARS
        {
            continue;
        }
        words.insert(word.to_string());
        if words.len() >= MAX_WORDS {
            break;
        }
    }
    words.into_iter().collect()
}

/// How well `query` matches `text`: lower is better, None is no match.
/// Exact-case prefix < case-insensitive prefix < in-order subsequence.
fn score(text: &str, query: &str) -> Option<u8> {
    if query.is_empty() || text.starts_with(query) {
        return Some(0);
    }
    let lower = text.to_lowercase();
    let wanted = query.to_lowercase();
    if lower.starts_with(&wanted) {
        return Some(1);
    }
    // In-order subsequence that starts with the same character: `cou`
    // finds `compute`, but `le` does not find `splice`.
    let mut chars = lower.chars();
    let mut wanted = wanted.chars();
    let first = wanted.next()?;
    (chars.next() == Some(first) && wanted.all(|q| chars.any(|c| c == q))).then_some(2)
}

/// Indices of `items` matching `query`, best first, at most `limit`.
/// Candidates identical to the query itself are left out.
pub fn rank(items: &[Candidate], query: &str, limit: usize) -> Vec<usize> {
    let mut hits: Vec<(u8, &str, usize, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, c)| c.insert != query || c.kind != "word")
        .filter_map(|(i, c)| {
            score(&c.filter, query).map(|s| (s, c.sort.as_str(), c.label.len(), i))
        })
        .collect();
    hits.sort();
    hits.into_iter().take(limit).map(|(.., i)| i).collect()
}

/// Plain text of an LSP snippet: `${1:name}` → `name`, `$1`/`$0` removed,
/// `\$` → `$`, choices `${1|a,b|}` → `a`.
pub fn strip_snippet(snippet: &str) -> String {
    let mut out = String::with_capacity(snippet.len());
    let mut chars = snippet.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '$' => match chars.peek() {
                Some(c) if c.is_ascii_digit() => {
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    }
                }
                Some('{') => {
                    chars.next();
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    }
                    let mut depth = 1;
                    let mut body = String::new();
                    let choice = chars.peek() == Some(&'|');
                    if matches!(chars.peek(), Some(':' | '|')) {
                        chars.next();
                    }
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                if let Some(n) = chars.next() {
                                    body.push(n);
                                }
                            }
                            '{' => {
                                depth += 1;
                                body.push(c);
                            }
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                body.push(c);
                            }
                            _ => body.push(c),
                        }
                    }
                    if choice {
                        body = body
                            .trim_end_matches('|')
                            .split(',')
                            .next()
                            .unwrap_or_default()
                            .into();
                    }
                    out.push_str(&strip_snippet(&body));
                }
                _ => out.push('$'),
            },
            _ => out.push(ch),
        }
    }
    out
}
