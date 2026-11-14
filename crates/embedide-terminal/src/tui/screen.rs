//! Row-diff writer. No source bytes are interpreted as terminal commands.
use super::layout::Frame;
pub use super::theme::Theme;
use super::theme::{ansi256, colors256, paint_color, run_colors, truecolor};
use crossterm::{
    cursor::{Hide, MoveTo, SetCursorStyle, Show},
    queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType},
};
use std::io::{self, Write};

/// Cached terminal frame and reusable output buffer.
#[derive(Default)]
pub struct Screen {
    previous: Option<Frame>,
    output: Vec<u8>,
    theme: Theme,
}
impl Screen {
    /// Emit changed rows and cursor only. Identical frames produce no I/O.
    pub fn draw(&mut self, frame: &Frame, out: &mut impl Write) -> io::Result<bool> {
        if self.previous.as_ref() == Some(frame) {
            return Ok(false);
        }
        self.output.clear();
        queue!(self.output, Hide)?;
        let resized = self
            .previous
            .as_ref()
            .is_none_or(|old| old.size != frame.size);
        if resized {
            queue!(self.output, Clear(ClearType::All))?;
        }
        for (index, row) in frame.rows.iter().enumerate() {
            if !resized && self.previous.as_ref().and_then(|old| old.rows.get(index)) == Some(row) {
                continue;
            }
            queue!(
                self.output,
                MoveTo(0, index as u16),
                SetAttribute(Attribute::Reset),
                Clear(ClearType::CurrentLine)
            )?;
            for run in &row.runs {
                // Text keeps the terminal's own colours (dark) or dark-on-light
                // (light). Bars, titles and selections ("chrome") use a brand
                // Carbon/Bone background instead of inverse video, which on a dark
                // terminal turns into a bright white block. Mono, and
                // terminals without 256 colours, keep inverse video.
                let (mut colors, chrome) = run_colors(self.theme, run.selected, colors256());
                if let Some(paint) = run.style {
                    colors.0 = paint_color(self.theme, paint);
                }
                // Programs in the integrated terminal keep their own colours
                // (like syntax colours, in every theme).
                let cell = run.cell.unwrap_or_default();
                let degrade = |color| match color {
                    Color::Rgb { r, g, b } if !truecolor() => Color::AnsiValue(ansi256(r, g, b)),
                    color => color,
                };
                colors.0 = cell.fg.map_or(colors.0, degrade);
                colors.1 = cell.bg.map_or(colors.1, degrade);
                queue!(self.output, SetAttribute(Attribute::Reset))?;
                for (on, attribute) in [
                    (
                        (run.selected && !chrome) || cell.inverse,
                        Attribute::Reverse,
                    ),
                    (cell.bold, Attribute::Bold),
                    (cell.dim, Attribute::Dim),
                    (cell.italic, Attribute::Italic),
                    (cell.underline, Attribute::Underlined),
                ] {
                    if on {
                        queue!(self.output, SetAttribute(attribute))?;
                    }
                }
                queue!(
                    self.output,
                    SetForegroundColor(colors.0),
                    SetBackgroundColor(colors.1),
                    Print(&run.text)
                )?;
            }
        }
        queue!(
            self.output,
            SetAttribute(Attribute::Reset),
            ResetColor,
            SetCursorStyle::DefaultUserShape
        )?;
        if let Some((x, y)) = frame.cursor {
            queue!(self.output, MoveTo(x, y), SetCursorStyle::SteadyBar)?;
            if !frame.insert {
                queue!(self.output, SetCursorStyle::SteadyBlock)?;
            }
            queue!(self.output, Show)?;
        }
        out.write_all(&self.output)?;
        out.flush()?;
        self.previous = Some(frame.clone());
        Ok(true)
    }
}

impl Screen {
    /// Change palette and force one full redraw.
    pub fn set_theme(&mut self, theme: Theme) {
        if self.theme != theme {
            self.theme = theme;
            self.previous = None;
        }
    }
}
