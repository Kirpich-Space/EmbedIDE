//! Row-diff writer. No source bytes are interpreted as terminal commands.
use super::layout::Frame;
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
                let colors = match self.theme {
                    Theme::Mono => (Color::Reset, Color::Reset),
                    Theme::Dark => (Color::Grey, Color::Black),
                    Theme::Light => (Color::Black, Color::White),
                };
                queue!(
                    self.output,
                    SetAttribute(if run.selected {
                        Attribute::Reverse
                    } else {
                        Attribute::Reset
                    }),
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

/// Restrained terminal palette. Monochrome uses only inverse video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Terminal default colors.
    #[default]
    Mono,
    /// Light text on a dark background.
    Dark,
    /// Dark text on a light background.
    Light,
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
