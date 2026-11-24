//! The EmbedIDE colour theme, in one place.
//!
//! Tokens come from the brand palette (`Desige/embedide-palette.json`):
//! `Carbon` (dark) and `Bone` (light). Frontend code never names a colour: it
//! marks text with a syntax category ([`Highlight`]) or a semantic UI [`Role`]
//! and the screen writer asks this module for the colour. Truecolor terminals
//! get the exact tokens, 256-colour terminals the nearest xterm colour, and
//! `Mono` (or terminals without 256 colours) keeps inverse-video bars.
use crossterm::style::Color;
use embedide_editor::syntax::Highlight;

/// Which palette the screen uses (`theme = "dark" | "light" | "mono"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Terminal default colors; bars and selections in inverse video.
    Mono,
    /// Carbon: the terminal's own background and text; Carbon bars (default).
    #[default]
    Dark,
    /// Bone: Carbon text on a Bone background; Bone bars and selections.
    Light,
}

/// A semantic UI colour, independent of syntax categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Rust brand text: prompts, focus markers, changed files. Used sparingly.
    Accent,
    /// Secondary text: hints, notes, tool output.
    Muted,
    /// A build, check or step that succeeded; added diff lines.
    Success,
    /// Compiler warnings, approvals waiting for the user, refusals.
    Warning,
    /// Errors and failures; removed diff lines. Never the brand colour.
    Error,
    /// Anything the AI proposed or is doing (cool, so it never looks like your code).
    Ai,
}

/// What a run of text is coloured as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// Source code category.
    Syntax(Highlight),
    /// UI meaning.
    Role(Role),
}
impl From<Highlight> for Paint {
    fn from(h: Highlight) -> Self {
        Self::Syntax(h)
    }
}
impl From<Role> for Paint {
    fn from(r: Role) -> Self {
        Self::Role(r)
    }
}

type Rgb = (u8, u8, u8);

/// Brand tokens of one theme.
#[derive(Debug)]
pub struct Tokens {
    /// `surface-0`: editor and window background.
    pub surface: Rgb,
    /// `surface-2`: bars, titles, selections, popups.
    pub raised: Rgb,
    /// `ink`: primary text.
    pub ink: Rgb,
    /// `ink-muted`: secondary text.
    pub muted: Rgb,
    /// `brand-text`: rust for text.
    pub accent: Rgb,
    /// `signal-ok`.
    pub success: Rgb,
    /// `signal-warn`.
    pub warning: Rgb,
    /// `signal-error`.
    pub error: Rgb,
    /// `signal-ai`.
    pub ai: Rgb,
    /// Syntax colours in [`Highlight`] order (see [`syntax_index`]).
    pub syntax: [Rgb; 11],
}

/// Carbon, the dark theme.
///
/// Syntax categories use cool blues, teals, greens, golds and purples so every
/// construct is distinct in both truecolor and 16-color TTYs; red is reserved
/// exclusively for [`Role::Error`].
pub const CARBON: Tokens = Tokens {
    surface: (0x0b, 0x0b, 0x0c),
    raised: (0x1c, 0x1c, 0x20),
    ink: (0xf4, 0xf1, 0xec),
    muted: (0xa8, 0xa6, 0xa2),
    accent: (0xe0, 0x70, 0x3f),
    success: (0x5d, 0xb5, 0x7a),
    warning: (0xe5, 0xae, 0x45),
    error: (0xf2, 0x64, 0x5f),
    ai: (0x86, 0xb9, 0xcc),
    syntax: [
        (0x82, 0xaa, 0xff), // keyword: periwinkle blue (never red)
        (0x5f, 0xc2, 0xb0), // type: teal
        (0xe5, 0xc0, 0x7b), // function: warm gold
        (0x89, 0xc7, 0x7d), // string: sage green
        (0xd8, 0xa6, 0x57), // number: soft amber
        (0x88, 0x8e, 0x9a), // comment: slate grey
        (0xc7, 0x92, 0xea), // constant: soft lavender
        (0xb0, 0xb4, 0xbc), // operator: light slate
        (0x66, 0xc2, 0xe0), // preprocessor: cyan-blue
        (0xf4, 0xf1, 0xec), // variable: warm bone ink
        (0xf0, 0xc6, 0x74), // branch: bright gold (distinct from keyword)
    ],
};

/// Bone, the light theme.
pub const BONE: Tokens = Tokens {
    surface: (0xf4, 0xf1, 0xec),
    raised: (0xea, 0xe5, 0xdc),
    ink: (0x0b, 0x0b, 0x0c),
    muted: (0x5c, 0x5a, 0x55),
    accent: (0xa3, 0x40, 0x1a),
    success: (0x27, 0x6b, 0x3e),
    warning: (0x7e, 0x55, 0x00),
    error: (0xb8, 0x30, 0x2c),
    ai: (0x2c, 0x6a, 0x80),
    syntax: [
        (0x1f, 0x4e, 0xa8), // keyword: cobalt blue (never red)
        (0x0e, 0x6b, 0x63), // type: deep teal
        (0x7a, 0x4b, 0x00), // function: bronze
        (0x2a, 0x6b, 0x2f), // string: forest green
        (0x6b, 0x47, 0x00), // number: dark amber
        (0x5a, 0x5e, 0x66), // comment: slate grey
        (0x6c, 0x2d, 0x8f), // constant: deep purple
        (0x4c, 0x50, 0x58), // operator: dark slate
        (0x1a, 0x60, 0x7a), // preprocessor: deep cyan
        (0x0b, 0x0b, 0x0c), // variable: carbon ink
        (0x85, 0x4a, 0x00), // branch: deep gold-brown
    ],
};

/// Tokens of `theme`. `Mono` colours text like `Dark` (it assumes the common
/// dark terminal) but keeps the terminal's own bars.
pub fn tokens(theme: Theme) -> &'static Tokens {
    if theme == Theme::Light {
        &BONE
    } else {
        &CARBON
    }
}

fn syntax_index(kind: Highlight) -> usize {
    match kind {
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
    }
}

/// The token behind a paint.
pub fn paint_rgb(theme: Theme, paint: Paint) -> Rgb {
    let t = tokens(theme);
    match paint {
        Paint::Syntax(kind) => t.syntax[syntax_index(kind)],
        Paint::Role(Role::Accent) => t.accent,
        Paint::Role(Role::Muted) => t.muted,
        Paint::Role(Role::Success) => t.success,
        Paint::Role(Role::Warning) => t.warning,
        Paint::Role(Role::Error) => t.error,
        Paint::Role(Role::Ai) => t.ai,
    }
}

/// Foreground colour of a paint on this terminal.
///
/// 16-colour TTY consoles (such as FreeBSD `vt` and the Linux console) receive
/// explicit ANSI-16 colours where red is strictly reserved for [`Role::Error`].
pub fn paint_color(theme: Theme, paint: Paint) -> Color {
    paint_color_for(theme, paint, truecolor(), colors256())
}

/// Foreground colour of a paint given explicit terminal colour capabilities.
pub fn paint_color_for(theme: Theme, paint: Paint, has_truecolor: bool, has_256: bool) -> Color {
    if !has_truecolor && !has_256 {
        return ansi16_paint(theme, paint);
    }
    let (r, g, b) = paint_rgb(theme, paint);
    if has_truecolor {
        Color::Rgb { r, g, b }
    } else {
        Color::AnsiValue(ansi256(r, g, b))
    }
}

/// Crisp 16-colour TTY mapping: red is used ONLY for [`Role::Error`].
fn ansi16_paint(theme: Theme, paint: Paint) -> Color {
    let light = theme == Theme::Light;
    match paint {
        Paint::Syntax(Highlight::Keyword) => {
            if light {
                Color::DarkBlue
            } else {
                Color::Blue
            }
        }
        Paint::Syntax(Highlight::Type) => {
            if light {
                Color::DarkCyan
            } else {
                Color::Cyan
            }
        }
        Paint::Syntax(Highlight::Function | Highlight::Branch) => {
            if light {
                Color::DarkYellow
            } else {
                Color::Yellow
            }
        }
        Paint::Syntax(Highlight::String) => {
            if light {
                Color::DarkGreen
            } else {
                Color::Green
            }
        }
        Paint::Syntax(Highlight::Number | Highlight::Constant) => {
            if light {
                Color::DarkMagenta
            } else {
                Color::Magenta
            }
        }
        Paint::Syntax(Highlight::Comment) => Color::DarkGrey,
        Paint::Syntax(Highlight::Operator) => {
            if light {
                Color::DarkGrey
            } else {
                Color::Grey
            }
        }
        Paint::Syntax(Highlight::Preprocessor) => {
            if light {
                Color::DarkCyan
            } else {
                Color::Cyan
            }
        }
        Paint::Syntax(Highlight::Variable) => Color::Reset,
        Paint::Role(Role::Accent) => {
            if light {
                Color::DarkYellow
            } else {
                Color::Yellow
            }
        }
        Paint::Role(Role::Muted) => Color::DarkGrey,
        Paint::Role(Role::Success) => {
            if light {
                Color::DarkGreen
            } else {
                Color::Green
            }
        }
        Paint::Role(Role::Warning) => {
            if light {
                Color::DarkYellow
            } else {
                Color::Yellow
            }
        }
        Paint::Role(Role::Error) => {
            if light {
                Color::DarkRed
            } else {
                Color::Red
            }
        }
        Paint::Role(Role::Ai) => {
            if light {
                Color::DarkCyan
            } else {
                Color::Cyan
            }
        }
    }
}

/// Foreground/background of a run, and whether it is drawn as "chrome"
/// (a Carbon/Bone bar instead of inverse video).
pub fn run_colors(theme: Theme, selected: bool, colors256: bool) -> ((Color, Color), bool) {
    let chrome = selected && theme != Theme::Mono && colors256;
    let t = tokens(theme);
    let c = |(r, g, b): Rgb| rgb(r, g, b);
    let colors = match (theme, chrome) {
        (Theme::Mono, _) | (Theme::Dark, false) => (Color::Reset, Color::Reset),
        (Theme::Dark, true) | (Theme::Light, true) => (c(t.ink), c(t.raised)),
        (Theme::Light, false) => (c(t.ink), c(t.surface)),
    };
    (colors, chrome)
}

/// A brand colour: truecolor where the terminal has it, the nearest
/// xterm-256 colour otherwise.
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    if truecolor() {
        Color::Rgb { r, g, b }
    } else {
        Color::AnsiValue(ansi256(r, g, b))
    }
}

/// The terminal shows the 256-colour palette (xterm-256color and friends,
/// or a truecolor COLORTERM); the Linux console and plain `xterm` do not.
pub fn colors256() -> bool {
    static COLORS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *COLORS.get_or_init(|| {
        truecolor()
            || std::env::var("TERM").is_ok_and(|t| {
                t.contains("256")
                    || t.contains("kitty")
                    || t.contains("direct")
                    || t == "foot"
                    || t.starts_with("wezterm")
                    || t == "alacritty"
            })
    })
}

/// `COLORTERM` announces 24-bit colour.
pub fn truecolor() -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2 contrast ratio.
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let lum = |(r, g, b): Rgb| {
            let ch = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.039_28 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_text_colour_is_readable_on_its_surfaces() {
        let roles = [
            Role::Accent,
            Role::Muted,
            Role::Success,
            Role::Warning,
            Role::Error,
            Role::Ai,
        ];
        for theme in [Theme::Dark, Theme::Light] {
            let t = tokens(theme);
            let mut paints: Vec<Paint> = roles.iter().map(|&r| r.into()).collect();
            paints.extend(
                [
                    Highlight::Keyword,
                    Highlight::Type,
                    Highlight::Function,
                    Highlight::String,
                    Highlight::Number,
                    Highlight::Comment,
                    Highlight::Constant,
                    Highlight::Operator,
                    Highlight::Preprocessor,
                    Highlight::Variable,
                    Highlight::Branch,
                ]
                .map(Paint::from),
            );
            for paint in paints {
                for (surface, name) in [(t.surface, "surface"), (t.raised, "raised")] {
                    let ratio = contrast(paint_rgb(theme, paint), surface);
                    assert!(
                        ratio >= 4.5,
                        "{theme:?} {paint:?} on {name}: {ratio:.2}:1 < 4.5:1"
                    );
                }
            }
            assert!(contrast(t.ink, t.raised) >= 7.0, "{theme:?} bar text");
        }
    }

    #[test]
    fn errors_warnings_and_success_are_distinct_and_not_the_brand() {
        for theme in [Theme::Dark, Theme::Light] {
            let t = tokens(theme);
            assert_ne!(
                paint_rgb(theme, Highlight::Branch.into()),
                paint_rgb(theme, Highlight::Keyword.into()),
                "{theme:?}: jumps keep their own colour"
            );
            let set = [t.accent, t.success, t.warning, t.error, t.ai];
            for (i, a) in set.iter().enumerate() {
                assert!(!set[i + 1..].contains(a), "{theme:?}: duplicate {a:?}");
            }
        }
    }

    #[test]
    fn brand_tokens_degrade_to_the_nearest_256_colour() {
        // Carbon bar and Bone text: dark and light greys of the xterm ramp.
        assert_eq!(ansi256(0x1c, 0x1c, 0x20), 234);
        assert_eq!(ansi256(0xf4, 0xf1, 0xec), 255);
        // Rust accent lands in the orange part of the cube, not on grey.
        let rust = ansi256(0xe0, 0x70, 0x3f);
        assert!((16..232).contains(&rust), "{rust}");
        assert_eq!(rust, 16 + 36 * 4 + 6 + 1);
        // …and stays apart from the error red (255,95,95) there.
        assert_ne!(rust, ansi256(0xf2, 0x64, 0x5f));
    }

    #[test]
    fn bars_use_brand_carbon_not_inverse_white_where_the_terminal_allows() {
        // Default theme: the terminal's own text colours, Carbon bars.
        assert_eq!(Theme::default(), Theme::Dark);
        assert_eq!(
            run_colors(Theme::Dark, false, true),
            ((Color::Reset, Color::Reset), false)
        );
        let ((fg, bg), chrome) = run_colors(Theme::Dark, true, true);
        assert!(chrome && fg == rgb(0xf4, 0xf1, 0xec) && bg == rgb(0x1c, 0x1c, 0x20));
        // Without 256 colours, and in mono, bars stay inverse video.
        assert!(!run_colors(Theme::Dark, true, false).1);
        assert!(!run_colors(Theme::Mono, true, true).1);
        assert!(run_colors(Theme::Light, true, true).1);
    }

    #[test]
    fn red_is_reserved_for_errors_in_tty_and_truecolor_syntax() {
        let kinds = [
            Highlight::Keyword,
            Highlight::Type,
            Highlight::Function,
            Highlight::String,
            Highlight::Number,
            Highlight::Comment,
            Highlight::Constant,
            Highlight::Operator,
            Highlight::Preprocessor,
            Highlight::Variable,
            Highlight::Branch,
        ];
        for theme in [Theme::Dark, Theme::Light] {
            for kind in kinds {
                let c16 = paint_color_for(theme, kind.into(), false, false);
                assert!(
                    !matches!(c16, Color::Red | Color::DarkRed),
                    "{theme:?} {kind:?} mapped to red in 16-color TTY: {c16:?}"
                );
                let (r, g, b) = paint_rgb(theme, kind.into());
                // Ensure no syntax token is red-dominant like an error colour.
                assert!(
                    !(r > 180 && r > g.saturating_add(70) && r > b.saturating_add(70)),
                    "{theme:?} {kind:?} looks red: ({r:#x}, {g:#x}, {b:#x})"
                );
            }
            assert!(matches!(
                paint_color_for(theme, Role::Error.into(), false, false),
                Color::Red | Color::DarkRed
            ));
            // Functions and variables are visually distinct in both themes.
            assert_ne!(
                paint_rgb(theme, Highlight::Function.into()),
                paint_rgb(theme, Highlight::Variable.into())
            );
        }
    }
}
