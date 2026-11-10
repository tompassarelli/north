//! The board as text: landings and boxes per hour first, then one panel per concern in the order the operator views
//! name them, each opening on its headline fact. At most `LINES` lines of at most `COLUMNS` characters.
use super::board::{self, window_words};
use super::Screen;
use crate::foundation::clock::Timestamp;

const LINES: usize = 50;
const COLUMNS: usize = 120;
const DETAIL_LINES: [usize; 4] = [8, 12, 8, 10];

pub fn screen(screen: &Screen, color: bool) -> String {
    let board = board::of(screen);
    let window = window_words(&screen.window);
    let mut lines = vec![board.first_line, format!("north dash: the last {window}, read at {}", Timestamp::from_unix_seconds(screen.now).local_twelve_hour())];
    for (panel, detail) in board.panels.iter().zip(DETAIL_LINES) {
        lines.push(String::new());
        lines.push(panel.title.to_owned());
        lines.push(format!("  {}", panel.headline));
        lines.extend(panel.lines.iter().take(detail).map(|line| format!("  {line}")));
        lines.extend(panel.unread.iter().map(|why| format!("  unread: {why}")));
    }
    lines
        .into_iter()
        .take(LINES)
        .map(|line| {
            let line: String = line.chars().take(COLUMNS).collect();
            if !color {
                return format!("{line}\n");
            }
            let tone = if line.starts_with("FAIL") || line.contains(" missed right now") {
                Some("31")
            } else if line.starts_with("pass") || line.contains(" is met right now") {
                Some("32")
            } else if line.contains("unread") || line.contains("slipping") {
                Some("33")
            } else {
                None
            };
            match tone {
                Some(tone) => format!("\x1b[{tone}m{line}\x1b[0m\n"),
                None if is_title(&line) => format!("\x1b[1m{line}\x1b[0m\n"),
                None => format!("{line}\n"),
            }
        })
        .collect()
}

fn is_title(line: &str) -> bool {
    matches!(line, "Work" | "Accounts" | "Infra" | "Health")
}
