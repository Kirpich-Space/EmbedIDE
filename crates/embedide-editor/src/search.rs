//! Streaming literal KMP search, including matches across piece boundaries.
use crate::{EditorError, TextBuffer};

fn matches<'a>(buffer: &'a TextBuffer, pattern: &'a str) -> impl Iterator<Item = usize> + 'a {
    let needle = pattern.as_bytes();
    let mut prefix = vec![0; needle.len()];
    let mut matched = 0;
    for index in 1..needle.len() {
        while matched > 0 && needle[index] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[index] == needle[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    let mut matched = 0;
    buffer
        .chunks()
        .flat_map(str::bytes)
        .enumerate()
        .filter_map(move |(index, byte)| {
            while matched > 0 && byte != needle[matched] {
                matched = prefix[matched - 1];
            }
            if byte == needle[matched] {
                matched += 1;
            }
            if matched == needle.len() {
                matched = prefix[matched - 1];
                Some(index + 1 - needle.len())
            } else {
                None
            }
        })
}

pub(crate) fn find(
    buffer: &TextBuffer,
    pattern: &str,
    cursor: usize,
    backward: bool,
    count: u32,
) -> Result<usize, EditorError> {
    if pattern.is_empty() {
        return Err(EditorError::NoSearch);
    }
    if pattern.contains(['\r', '\n']) {
        return Err(EditorError::Unsupported(
            "multi-line literal search is not implemented",
        ));
    }
    let mut total = 0;
    let mut before = 0;
    let mut through = 0;
    for position in matches(buffer, pattern) {
        total += 1;
        if position < cursor {
            before += 1;
        }
        if position <= cursor {
            through += 1;
        }
    }
    if total == 0 {
        return Err(EditorError::SearchNotFound(pattern.into()));
    }
    let steps = count.saturating_sub(1) as usize % total;
    let index = if backward {
        (before + total - 1 + total - steps) % total
    } else {
        (through + steps) % total
    };
    matches(buffer, pattern)
        .nth(index)
        .ok_or_else(|| EditorError::SearchNotFound(pattern.into()))
}
