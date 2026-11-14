use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::fleet::model::Status;

use super::screen::{Focus, Screen};

const MUTED: Style = Style::new().fg(Color::DarkGray);

pub fn status_style(status: Status) -> Style {
    match status {
        Status::Working => Style::default().fg(Color::Green),
        Status::Starting => Style::default().fg(Color::Cyan),
        Status::Idle => Style::default().fg(Color::Gray),
        Status::Parked => Style::default().fg(Color::Blue),
        Status::Exited => Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
    }
}

pub fn draw_strip(frame: &mut Frame<'_>, screen: &Screen, area: Rect) {
    let focus = match screen.focus {
        Focus::List => "keys: session list",
        Focus::Composer => "keys: composer",
    };
    let focus_width = u16::try_from(focus.chars().count() + 1).unwrap_or(u16::MAX).min(area.width);
    let left = Rect { width: area.width - focus_width, ..area };
    let right = Rect { x: area.x + left.width, width: focus_width, ..area };
    frame.render_widget(Paragraph::new(Span::styled(focus, Style::default().fg(Color::Cyan))).alignment(Alignment::Right), right);
    if screen.sessions.is_empty() {
        let hint = "no sessions yet · Ctrl+N starts one";
        frame.render_widget(Paragraph::new(Span::styled(hint, MUTED)), left);
        return;
    }
    let cells: Vec<Vec<Span<'static>>> = screen
        .sessions
        .iter()
        .map(|session| {
            let marked = screen.selected.as_deref() == Some(session.record.name.as_str());
            let name_style = if marked { Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED) } else { Style::default() };
            vec![
                Span::styled(format!("{}{}", if marked { "▸" } else { " " }, session.record.name), name_style),
                Span::styled(format!(" {}  ", session.phase.label()), status_style(session.status)),
            ]
        })
        .collect();
    let widths: Vec<usize> = cells.iter().map(|cell| cell.iter().map(Span::width).sum()).collect();
    let selected = screen.sessions.iter().position(|session| screen.selected.as_deref() == Some(session.record.name.as_str())).unwrap_or(0);
    let room = usize::from(left.width);
    let mut start = 0;
    while start < selected && widths[start..=selected].iter().sum::<usize>() > room {
        start += 1;
    }
    let spans: Vec<Span<'static>> = cells.into_iter().skip(start).flatten().collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), left);
}
