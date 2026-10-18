//! Bounded append-only snapshots. No terminal escape sequences or screen state.
use crate::{FrontendError, LineEditor};
use std::io::{self, Write};

fn safe_char(out: &mut impl Write, ch: char) -> io::Result<()> {
    if ch.is_control() {
        write!(out, "{}", ch.escape_default())
    } else {
        write!(out, "{ch}")
    }
}

fn safe_text(out: &mut impl Write, value: &str, limit: usize) -> io::Result<()> {
    for ch in value.chars().take(limit) {
        safe_char(out, ch)?;
    }
    Ok(())
}

pub(crate) fn draw(session: &LineEditor, out: &mut impl Write) -> Result<(), FrontendError> {
    let editor = session.editor();
    let state = editor.state();
    let position = state.position()?;
    write!(out, "\n[{:?}] ", editor.input().mode())?;
    if let Some(path) = state.path() {
        safe_text(out, &path.to_string_lossy(), 160)?;
    } else {
        write!(out, "[untitled]")?;
    }
    writeln!(
        out,
        "{}  {}:{} (Unicode scalars)",
        if state.is_modified() { " *" } else { "" },
        position.line + 1,
        position.scalar_column + 1
    )?;
    // Center the bounded viewport around the cursor; range lookup is indexed.
    let top = position.line.saturating_sub(5);
    let left = position.scalar_column.saturating_sub(50);
    for line in top..top.saturating_add(12).min(state.line_count()) {
        write!(
            out,
            "{} {:>6} | ",
            if line == position.line { '>' } else { ' ' },
            line + 1
        )?;
        if left > 0 {
            write!(out, "…")?;
        }
        if let Some(range) = state.line_range(line) {
            let mut offset = range.start;
            for (column, ch) in state
                .buffer()
                .chunks_in(range)?
                .flat_map(str::chars)
                .enumerate()
            {
                if column >= left.saturating_add(100) {
                    write!(out, "…")?;
                    break;
                }
                if column >= left {
                    if line == position.line && offset == state.cursor() {
                        write!(out, "▏")?;
                    }
                    safe_char(out, ch)?;
                }
                offset += ch.len_utf8();
            }
            if line == position.line && offset == state.cursor() {
                write!(out, "▏")?;
            }
        }
        writeln!(out)?;
    }
    if let Some(prompt) = editor.input().command_prompt() {
        write!(out, "command: {prompt}")?;
        safe_text(out, editor.input().command_text().unwrap_or(""), 160)?;
        writeln!(out)?;
    }
    if !session.message.is_empty() {
        safe_text(out, &session.message, 512)?;
        writeln!(out)?;
    }
    write!(out, "keys> ")?;
    out.flush()?;
    Ok(())
}
