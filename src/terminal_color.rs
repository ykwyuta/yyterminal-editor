//! Lowers 24-bit colors to what the terminal declares it can show.
//!
//! Terminals without truecolor may read `48;2;R;G;B` as separate SGR codes
//! (for example `36` and `46`), painting text in its own background color.
use ratatui::{buffer::Buffer, style::Color};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    TrueColor,
    Indexed256,
    Basic16,
}

/// Reads `COLORTERM` and `TERM` of the current process.
pub fn detect() -> Depth {
    detect_from(
        std::env::var("COLORTERM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
        cfg!(windows),
    )
}

pub fn detect_from(colorterm: Option<&str>, term: Option<&str>, windows: bool) -> Depth {
    let colorterm = colorterm.unwrap_or("").to_ascii_lowercase();
    if colorterm == "truecolor" || colorterm == "24bit" {
        return Depth::TrueColor;
    }
    let term = term.unwrap_or("").to_ascii_lowercase();
    if term.ends_with("-direct") || term.contains("truecolor") || term.contains("24bit") {
        Depth::TrueColor
    } else if term.contains("256color") {
        Depth::Indexed256
    } else if term.is_empty() && windows {
        // Windows consoles with virtual terminal processing accept 24-bit colors.
        Depth::TrueColor
    } else {
        Depth::Basic16
    }
}

// xterm's default palette for the 16 named colors.
const BASIC: [(Color, [u8; 3]); 16] = [
    (Color::Black, [0, 0, 0]),
    (Color::Red, [205, 0, 0]),
    (Color::Green, [0, 205, 0]),
    (Color::Yellow, [205, 205, 0]),
    (Color::Blue, [0, 0, 238]),
    (Color::Magenta, [205, 0, 205]),
    (Color::Cyan, [0, 205, 205]),
    (Color::Gray, [229, 229, 229]),
    (Color::DarkGray, [127, 127, 127]),
    (Color::LightRed, [255, 0, 0]),
    (Color::LightGreen, [0, 255, 0]),
    (Color::LightYellow, [255, 255, 0]),
    (Color::LightBlue, [92, 92, 255]),
    (Color::LightMagenta, [255, 0, 255]),
    (Color::LightCyan, [0, 255, 255]),
    (Color::White, [255, 255, 255]),
];
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(&x, y)| (i32::from(x) - i32::from(y)).pow(2) as u32)
        .sum()
}

fn indexed_rgb(index: u8) -> [u8; 3] {
    match index {
        0..16 => BASIC[usize::from(index)].1,
        16..232 => {
            let n = index - 16;
            [
                CUBE[usize::from(n / 36)],
                CUBE[usize::from(n / 6 % 6)],
                CUBE[usize::from(n % 6)],
            ]
        }
        _ => [8 + 10 * (index - 232); 3],
    }
}

fn to_256(rgb: [u8; 3]) -> u8 {
    let level = |v: u8| {
        (0..6)
            .min_by_key(|&i| (i32::from(CUBE[i]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let cube = 16 + 36 * level(rgb[0]) + 6 * level(rgb[1]) + level(rgb[2]);
    let average = (rgb.iter().map(|&v| u32::from(v)).sum::<u32>() / 3) as u8;
    let gray = 232 + (average.saturating_sub(3) / 10).min(23);
    [cube as u8, gray]
        .into_iter()
        .min_by_key(|&i| distance(indexed_rgb(i), rgb))
        .unwrap_or(16)
}

fn to_16(rgb: [u8; 3]) -> Color {
    BASIC
        .iter()
        .min_by_key(|(_, c)| distance(*c, rgb))
        .map_or(Color::Reset, |(color, _)| *color)
}

pub fn adapt(color: Color, depth: Depth) -> Color {
    match (color, depth) {
        (_, Depth::TrueColor) => color,
        (Color::Rgb(r, g, b), Depth::Indexed256) => Color::Indexed(to_256([r, g, b])),
        (Color::Rgb(r, g, b), Depth::Basic16) => to_16([r, g, b]),
        (Color::Indexed(i), Depth::Basic16) => to_16(indexed_rgb(i)),
        _ => color,
    }
}

fn is_dark(color: Color) -> bool {
    BASIC
        .iter()
        .find(|(c, _)| *c == color)
        .is_some_and(|(_, [r, g, b])| {
            u32::from(*r) * 299 + u32::from(*g) * 587 + u32::from(*b) * 114 < 128_000
        })
}

/// Converts every cell; text never ends up in the same color as its background.
pub fn adapt_buffer(buffer: &mut Buffer, depth: Depth) {
    if depth == Depth::TrueColor {
        return;
    }
    for cell in &mut buffer.content {
        let (fg, bg) = (cell.fg, cell.bg);
        cell.fg = adapt(fg, depth);
        cell.bg = adapt(bg, depth);
        if cell.fg == cell.bg && fg != bg && cell.fg != Color::Reset {
            cell.fg = if depth == Depth::Basic16 && !is_dark(cell.bg) {
                Color::Black
            } else if depth == Depth::Basic16 {
                Color::White
            } else {
                // Two RGB colors met on one palette entry: use the contrasting gray end.
                let [r, g, b] = indexed_rgb(match cell.bg {
                    Color::Indexed(i) => i,
                    _ => 0,
                });
                let light = u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 >= 128_000;
                Color::Indexed(if light { 16 } else { 231 })
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Cell, layout::Rect};

    #[test]
    fn depth_follows_colorterm_and_term() {
        assert_eq!(
            detect_from(Some("truecolor"), Some("xterm"), false),
            Depth::TrueColor
        );
        assert_eq!(detect_from(Some("24bit"), None, false), Depth::TrueColor);
        assert_eq!(
            detect_from(None, Some("xterm-direct"), false),
            Depth::TrueColor
        );
        assert_eq!(
            detect_from(None, Some("xterm-256color"), false),
            Depth::Indexed256
        );
        assert_eq!(
            detect_from(None, Some("screen-256color"), false),
            Depth::Indexed256
        );
        assert_eq!(detect_from(None, Some("linux"), false), Depth::Basic16);
        assert_eq!(detect_from(None, Some("xterm"), false), Depth::Basic16);
        assert_eq!(detect_from(None, None, false), Depth::Basic16);
        assert_eq!(detect_from(None, None, true), Depth::TrueColor);
        assert_eq!(
            detect_from(None, Some("xterm-256color"), true),
            Depth::Indexed256
        );
    }

    #[test]
    fn rgb_maps_to_nearest_palette_entry() {
        // The Markdown code background stays a dark gray, not a cyan read from its digits.
        assert_eq!(
            adapt(Color::Rgb(30, 36, 46), Depth::Indexed256),
            Color::Indexed(235)
        );
        assert_eq!(adapt(Color::Rgb(30, 36, 46), Depth::Basic16), Color::Black);
        assert_eq!(
            adapt(Color::Rgb(255, 0, 0), Depth::Indexed256),
            Color::Indexed(196)
        );
        assert_eq!(
            adapt(Color::Rgb(135, 175, 215), Depth::Indexed256),
            Color::Indexed(110)
        );
        assert_eq!(
            adapt(Color::Rgb(128, 128, 128), Depth::Indexed256),
            Color::Indexed(244)
        );
        assert_eq!(adapt(Color::Indexed(196), Depth::Basic16), Color::LightRed);
        assert_eq!(
            adapt(Color::Indexed(196), Depth::Indexed256),
            Color::Indexed(196)
        );
        assert_eq!(adapt(Color::Yellow, Depth::Basic16), Color::Yellow);
        assert_eq!(
            adapt(Color::Rgb(1, 2, 3), Depth::TrueColor),
            Color::Rgb(1, 2, 3)
        );
        for i in 0..=255u8 {
            assert_eq!(
                to_256(indexed_rgb(i)),
                to_256(indexed_rgb(to_256(indexed_rgb(i))))
            );
        }
    }

    #[test]
    fn buffer_keeps_text_distinct_from_background() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        let cell = |fg, bg| {
            let mut c = Cell::default();
            c.set_symbol("x").set_fg(fg).set_bg(bg);
            c
        };
        buffer.content = vec![
            cell(Color::Yellow, Color::Rgb(30, 36, 46)),
            cell(Color::Rgb(40, 40, 40), Color::Rgb(20, 20, 20)),
            cell(Color::Rgb(250, 250, 250), Color::White),
            cell(Color::Reset, Color::Reset),
        ];
        let mut basic = buffer.clone();
        adapt_buffer(&mut basic, Depth::Basic16);
        let pairs: Vec<_> = basic.content.iter().map(|c| (c.fg, c.bg)).collect();
        assert_eq!(
            pairs,
            [
                (Color::Yellow, Color::Black),
                (Color::White, Color::Black),
                (Color::Black, Color::White),
                (Color::Reset, Color::Reset),
            ]
        );
        adapt_buffer(&mut buffer, Depth::Indexed256);
        assert_eq!(
            (buffer.content[0].fg, buffer.content[0].bg),
            (Color::Yellow, Color::Indexed(235))
        );
        for c in &buffer.content[..3] {
            assert_ne!(c.fg, c.bg);
            assert!(!matches!(c.fg, Color::Rgb(..)) && !matches!(c.bg, Color::Rgb(..)));
        }
    }
}
