//! Transport notation only; modal interpretation belongs to VimInput.
use embedide_editor::{vim::Key, EditorError};

pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024;

pub(crate) fn decode(line: &str) -> Result<Vec<Key>, EditorError> {
    let mut keys = Vec::new();
    let mut remaining = line;
    while let Some(ch) = remaining.chars().next() {
        if remaining.starts_with("\u{1b}[") || remaining.starts_with("\u{1b}O") {
            let invalid = || {
                EditorError::Unsupported(
                    "unsupported terminal escape sequence; use explicit key notation",
                )
            };
            let end = remaining[2..]
                .bytes()
                .take(32)
                .position(|byte| (0x40..=0x7e).contains(&byte))
                .ok_or_else(invalid)?
                + 3;
            let key = match &remaining[..end] {
                "\u{1b}[A" | "\u{1b}OA" => Key::Up,
                "\u{1b}[B" | "\u{1b}OB" => Key::Down,
                "\u{1b}[C" | "\u{1b}OC" => Key::Right,
                "\u{1b}[D" | "\u{1b}OD" => Key::Left,
                "\u{1b}[H" | "\u{1b}OH" | "\u{1b}[1~" | "\u{1b}[7~" => Key::Home,
                "\u{1b}[F" | "\u{1b}OF" | "\u{1b}[4~" | "\u{1b}[8~" => Key::End,
                "\u{1b}[3~" => Key::Delete,
                _ => return Err(invalid()),
            };
            keys.push(key);
            remaining = &remaining[end..];
            continue;
        }
        if remaining.starts_with('<') {
            if let Some(end) = remaining.bytes().take(8).position(|byte| byte == b'>') {
                let key = match &remaining[..=end] {
                    "<lt>" => Some(Key::Char('<')),
                    "<Esc>" => Some(Key::Escape),
                    "<Enter>" | "<CR>" => Some(Key::Enter),
                    "<Tab>" => Some(Key::Tab),
                    "<S-Tab>" => Some(Key::BackTab),
                    "<BS>" => Some(Key::Backspace),
                    "<Del>" => Some(Key::Delete),
                    "<C-r>" => Some(Key::Ctrl('r')),
                    "<Left>" => Some(Key::Left),
                    "<Right>" => Some(Key::Right),
                    "<Up>" => Some(Key::Up),
                    "<Down>" => Some(Key::Down),
                    "<Home>" => Some(Key::Home),
                    "<End>" => Some(Key::End),
                    _ => None,
                };
                if let Some(key) = key {
                    keys.push(key);
                    remaining = &remaining[end + 1..];
                    continue;
                }
            }
        }
        keys.push(match ch {
            '\u{1b}' => Key::Escape,
            '\u{12}' => Key::Ctrl('r'),
            '\t' => Key::Tab,
            '\u{8}' | '\u{7f}' => Key::Backspace,
            _ => Key::Char(ch),
        });
        remaining = &remaining[ch.len_utf8()..];
    }
    Ok(keys)
}
