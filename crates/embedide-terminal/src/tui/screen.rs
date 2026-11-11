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
use embedide_editor::syntax::Highlight;
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
                let mut colors = match self.theme {
                    Theme::Mono => (Color::Reset, Color::Reset),
                    Theme::Dark => (Color::Grey, Color::Black),
                    Theme::Light => (Color::Black, Color::White),
                };
                if let Some(kind) = run.style {
                    colors.0 = syntax_color(self.theme, kind);
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
                    (run.selected || cell.inverse, Attribute::Reverse),
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

/// Syntax colours: a restrained Tomorrow-style palette. `Mono` keeps the
/// terminal's own background and text colour and assumes a dark background
/// (the common case); `Light` uses darker tones for contrast on white.
pub fn syntax_color(theme: Theme, kind: Highlight) -> Color {
    let dark = [
        (0xb2, 0x94, 0xbb), // keyword: muted violet
        (0xe6, 0xc0, 0x7b), // type: soft amber
        (0x81, 0xa2, 0xbe), // function: steel blue
        (0xa8, 0xc0, 0x6a), // string: sage green
        (0xde, 0x93, 0x5f), // number: orange
        (0x80, 0x86, 0x8e), // comment: grey
        (0xd7, 0x7b, 0x7b), // constant: rose red
        (0x8a, 0xbe, 0xb7), // operator: teal
        (0xc9, 0x8c, 0xb4), // preprocessor: mauve
        (0xe0, 0x8a, 0x6e), // variable: coral
        (0xf0, 0xa0, 0x5a), // branch: bright orange (jumps stand out)
    ];
    let light = [
        (0x7a, 0x3e, 0x9d),
        (0x8a, 0x5a, 0x00),
        (0x2b, 0x5b, 0x9e),
        (0x4d, 0x7a, 0x0a),
        (0xb3, 0x52, 0x00),
        (0x70, 0x72, 0x76),
        (0xb0, 0x22, 0x22),
        (0x1f, 0x7a, 0x80),
        (0x9b, 0x2f, 0x76),
        (0xa8, 0x40, 0x1e),
        (0xc0, 0x50, 0x00),
    ];
    let index = match kind {
        Highlight::Keyword => 0,
        Highlight::Type => 1,
        Highlight::Function => 2,
        Highlight::String => 3,
        Highlight::Number => 4,
        Highlight::Comment => 5,
        Highlight::Constant => 6,
        Highlight::Operator => 7,
        Highlight::Preprocessor => 8,
        Highlight::Variable => 9,
        Highlight::Branch => 10,
    };
    let (r, g, b) = if theme == Theme::Light {
        light[index]
    } else {
        dark[index]
    };
    if truecolor() {
        Color::Rgb { r, g, b }
    } else {
        Color::AnsiValue(ansi256(r, g, b))
    }
}

fn truecolor() -> bool {
    static TRUECOLOR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRUECOLOR
        .get_or_init(|| std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit"))
}

/// Nearest colour in the xterm 6×6×6 cube or the 24-step grey ramp.
pub fn ansi256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |v: u8| {
        (0..6)
            .min_by_key(|&i| (i32::from(LEVELS[i]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let average = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let grey_index = ((average.saturating_sub(8)) / 10).min(23) as u8;
    let grey = 8 + 10 * grey_index;
    let distance = |(x, y, z): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(x, r) + d(y, g) + d(z, b)
    };
    if distance((grey, grey, grey)) < distance(cube) {
        232 + grey_index
    } else {
        16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8
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
