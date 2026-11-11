use std::fmt::Write;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

const NAMED: [(Color, u8); 16] = [
    (Color::Black, 0),
    (Color::Red, 1),
    (Color::Green, 2),
    (Color::Yellow, 3),
    (Color::Blue, 4),
    (Color::Magenta, 5),
    (Color::Cyan, 6),
    (Color::Gray, 7),
    (Color::DarkGray, 60),
    (Color::LightRed, 61),
    (Color::LightGreen, 62),
    (Color::LightYellow, 63),
    (Color::LightBlue, 64),
    (Color::LightMagenta, 65),
    (Color::LightCyan, 66),
    (Color::White, 67),
];

fn color(color: Color, background: bool) -> Option<String> {
    let base: u8 = if background { 40 } else { 30 };
    if let Some((_, offset)) = NAMED.iter().find(|(named, _)| *named == color) {
        return Some((base + offset).to_string());
    }
    match color {
        Color::Indexed(index) => Some(format!("{};5;{index}", base + 8)),
        Color::Rgb(red, green, blue) => Some(format!("{};2;{red};{green};{blue}", base + 8)),
        _ => None,
    }
}

fn sgr(foreground: Color, background: Color, modifier: Modifier) -> String {
    let mut codes = vec!["0".to_owned()];
    if modifier.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if modifier.contains(Modifier::DIM) {
        codes.push("2".into());
    }
    if modifier.contains(Modifier::REVERSED) {
        codes.push("7".into());
    }
    codes.extend(color(foreground, false));
    codes.extend(color(background, true));
    format!("\x1b[{}m", codes.join(";"))
}

pub fn render(buffer: &Buffer, ansi: bool) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        let mut current = None;
        let mut line = String::new();
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            if ansi {
                let style = (cell.fg, cell.bg, cell.modifier);
                if current != Some(style) {
                    line.push_str(&sgr(cell.fg, cell.bg, cell.modifier));
                    current = Some(style);
                }
            }
            line.push_str(cell.symbol());
        }
        let line = if ansi { line } else { line.trim_end().to_owned() };
        let _ = writeln!(out, "{line}{}", if ansi { "\x1b[0m" } else { "" });
    }
    out
}
