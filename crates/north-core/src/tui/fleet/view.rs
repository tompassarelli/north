use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::fleet::model::{Status, Transcript};

use super::desk::SessionSummary;
use super::form::{Field, LaunchForm};
use super::screen::{Dialog, Focus, Notice, Screen};
use super::digest_view::{draw_digest, header_lines, span};
use super::painter::TranscriptPainter;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const COMPOSER: Style = Style::new().fg(Color::Rgb(229, 231, 235)).bg(Color::Rgb(37, 39, 45));
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(frame: &mut Frame<'_>, screen: &mut Screen, transcript: Option<&Transcript>, painter: &mut TranscriptPainter, now_ms: u64) {
    let width = frame.area().width.saturating_sub(4).max(1);
    let composer_rows = screen.composer.measure(width).preferred_rows.clamp(1, 8);
    let header = header_lines(screen.accounts(), |account| screen.digest.quota(account, &screen.sessions), usize::from(frame.area().width));
    let header_rows = u16::try_from(header.len()).unwrap_or(u16::MAX);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_rows), Constraint::Min(3), Constraint::Length(composer_rows + 2), Constraint::Length(1)])
        .split(frame.area());
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Min(20)])
        .split(rows[1]);
    frame.render_widget(Paragraph::new(header), rows[0]);
    draw_sessions(frame, screen, panes[0], now_ms);
    if screen.digest.shown {
        draw_digest(frame, screen, panes[1], now_ms);
    } else {
        draw_transcript(frame, screen, transcript, painter, panes[1]);
    }
    draw_composer(frame, screen, rows[2]);
    draw_status(frame, screen, rows[3]);
    if screen.focus == Focus::Composer && screen.dialog.is_none() {
        screen.slash.draw(frame, rows[2], &screen.composer.lines().join("\n"));
    }
    match &screen.dialog {
        Some(Dialog::Launch(form)) => draw_launch_form(frame, form),
        Some(Dialog::ConfirmStop(name)) => draw_confirm_stop(frame, name),
        None => {}
    }
}

fn pane(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused { Style::default().fg(Color::Cyan) } else { MUTED })
        .title(title)
}

fn draw_sessions(frame: &mut Frame<'_>, screen: &Screen, area: Rect, now_ms: u64) {
    let title = Line::from(vec![Span::styled(" sessions ", Style::default().add_modifier(Modifier::BOLD)), Span::styled(format!("{} ", screen.sessions.len()), MUTED)]);
    let block = pane(title, screen.focus == Focus::List && screen.dialog.is_none());
    if screen.sessions.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled("no sessions · Ctrl+N starts one", MUTED)).block(block).wrap(Wrap { trim: true }), area);
        return;
    }
    let items: Vec<ListItem<'static>> = screen.sessions.iter().map(|session| session_item(session, marker(screen, &session.record.name, now_ms), now_ms)).collect();
    let mut state = ListState::default();
    state.select(screen.selected.as_deref().and_then(|name| screen.sessions.iter().position(|session| session.record.name == name)));
    let list = List::new(items).block(block).highlight_style(Style::default().bg(Color::Rgb(52, 58, 62)));
    frame.render_stateful_widget(list, area, &mut state);
}

fn marker(screen: &Screen, name: &str, now_ms: u64) -> Span<'static> {
    if screen.busy.contains_key(name) {
        let frame = SPINNER[usize::try_from(now_ms / 120).unwrap_or(0) % SPINNER.len()];
        Span::styled(format!("{frame} "), Style::default().fg(Color::Cyan))
    } else if screen.unread.contains(name) {
        Span::styled("● ", Style::default().fg(Color::Magenta))
    } else {
        Span::raw("  ")
    }
}

fn session_item(session: &SessionSummary, marker: Span<'static>, now_ms: u64) -> ListItem<'static> {
    let record = &session.record;
    let headline = Line::from(vec![
        marker,
        Span::raw("  ".repeat(session.depth)),
        Span::styled(record.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(session.status.label(), status_style(session.status)),
        Span::styled(format!(" · {}", span(now_ms.saturating_sub(record.created_ms))), MUTED),
    ]);
    let directory = record.cwd.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| record.cwd.display().to_string());
    let mut detail = vec![Span::raw("  ".repeat(session.depth + 1))];
    if session.status == Status::Exited {
        detail.push(Span::styled("type to resume · ", Style::default().fg(Color::Yellow)));
    }
    detail.push(Span::styled(format!("{}·{} · {directory}", record.provider.label(), record.account), MUTED));
    ListItem::new(vec![headline, Line::from(detail)])
}

pub fn status_style(status: Status) -> Style {
    match status {
        Status::Working => Style::default().fg(Color::Green),
        Status::Starting => Style::default().fg(Color::Cyan),
        Status::Idle => Style::default().fg(Color::Gray),
        Status::Parked => Style::default().fg(Color::Blue),
        Status::Exited => Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
    }
}

pub fn age(now_ms: u64, then_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(then_ms) / 60_000;
    match minutes {
        0 => "0m".into(),
        1..=59 => format!("{minutes}m"),
        60..=1439 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

fn draw_transcript(frame: &mut Frame<'_>, screen: &mut Screen, transcript: Option<&Transcript>, painter: &mut TranscriptPainter, area: Rect) {
    let Some(session) = screen.selected_session() else {
        frame.render_widget(pane(Line::from(" transcript "), false), area);
        return;
    };
    let record = &session.record;
    let mut title = vec![
        Span::styled(format!(" {} ", record.name), Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(format!("{} ", session.status.label()), status_style(session.status)),
        Span::styled(format!("· {} ", record.cwd.display()), MUTED),
    ];
    if let Some(model) = &record.model {
        title.push(Span::styled(format!("· {model} "), MUTED));
    }
    if screen.scroll > 0 {
        title.push(Span::styled(format!("· ↑{} ", screen.scroll), Style::default().fg(Color::Yellow)));
    }
    let block = pane(Line::from(title), false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let pending = screen.pending_for(&record.name);
    let lines = painter.lines(&record.name, transcript, &pending, inner.width);
    let width = usize::from(inner.width.max(1));
    let count: usize = lines.iter().map(|line| line.width().max(1).div_ceil(width)).sum();
    let limit = u16::try_from(count.saturating_sub(usize::from(inner.height))).unwrap_or(u16::MAX);
    screen.measure_viewport(inner.height, limit);
    let text = Text::from(lines.to_vec());
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).scroll((limit - screen.scroll, 0)), inner);
}

fn draw_composer(frame: &mut Frame<'_>, screen: &Screen, area: Rect) {
    let gate = screen.digest.selected().filter(|_| screen.digest.shown);
    let target = match (gate, screen.selected_session()) {
        (Some(gate), _) => format!(" answer gate {} for {} ", gate.id, gate.request.session),
        (None, Some(session)) if session.status == Status::Exited => format!(" resume {} ", session.record.name),
        (None, Some(session)) => format!(" message {} ", session.record.name),
        (None, None) => " no session ".to_owned(),
    };
    let block = pane(Line::from(target), screen.focus == Focus::Composer && screen.dialog.is_none()).style(COMPOSER);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Span::styled("❯ ", COMPOSER.fg(Color::Cyan))), Rect { width: inner.width.min(2), ..inner });
    if inner.width > 2 {
        frame.render_widget(&screen.composer, Rect { x: inner.x + 2, width: inner.width - 2, ..inner });
    }
}

fn draw_status(frame: &mut Frame<'_>, screen: &Screen, area: Rect) {
    let line = match &screen.notice {
        Some(Notice::Error(message)) => Line::from(Span::styled(message.clone(), Style::default().fg(Color::Red))),
        Some(Notice::Info(message)) => Line::from(Span::styled(message.clone(), Style::default().fg(Color::Yellow))),
        None => hints(screen, usize::from(area.width)),
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn hints(screen: &Screen, width: usize) -> Line<'static> {
    let answering = screen.digest.shown && screen.digest.selected().is_some();
    let pairs: &[(&str, &str)] = match (screen.focus, answering) {
        (Focus::List, true) => &[("j/k", "gate"), ("1-9", "answer with that option"), ("type", "answer in your words"), ("^T", "transcript"), ("Tab", "composer"), ("^Q", "quit")],
        (Focus::Composer, true) => &[("Enter", "answer the gate"), ("Alt+Enter", "newline"), ("Tab", "gates"), ("^T", "transcript"), ("^Q", "quit")],
        (Focus::List, false) => &[("j/k", "select"), ("r", "resume"), ("^T", "digest"), ("Tab", "composer"), ("PgUp/PgDn", "scroll"), ("^N", "new"), ("^C", "interrupt"), ("^X", "stop"), ("^D", "forget"), ("^Q", "quit")],
        (Focus::Composer, false) => &[("Enter", "send"), ("Alt+Enter", "newline"), ("^Enter/Alt+U", "urgent"), ("Tab", "list"), ("^T", "digest"), ("^G", "deepen and send"), ("^C", "interrupt"), ("^X", "stop"), ("^N", "new"), ("^Q", "quit")],
    };
    let mut spans = Vec::new();
    let mut used = 0;
    for (index, (key, action)) in pairs.iter().enumerate() {
        let separator = if index > 0 { " · " } else { "" };
        let cost = separator.chars().count() + key.chars().count() + 1 + action.chars().count();
        if used + cost > width {
            break;
        }
        used += cost;
        if index > 0 {
            spans.push(Span::styled(separator, MUTED));
        }
        spans.push(Span::styled(*key, Style::default().fg(Color::Cyan)));
        spans.push(Span::styled(format!(" {action}"), MUTED));
    }
    Line::from(spans)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 2, width, height }
}

fn draw_confirm_stop(frame: &mut Frame<'_>, name: &str) {
    let area = centered(frame.area(), 48, 5);
    frame.render_widget(Clear, area);
    let text = vec![Line::from(format!("Stop {name}?")), Line::from(Span::styled("y stops · n or Esc keeps it running", MUTED))];
    frame.render_widget(Paragraph::new(text).block(pane(Line::from(" stop session "), true)).wrap(Wrap { trim: true }), area);
}

fn draw_launch_form(frame: &mut Frame<'_>, form: &LaunchForm) {
    let area = centered(frame.area(), 72, 18);
    frame.render_widget(Clear, area);
    let block = pane(Line::from(" new session "), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(inner);
    for (index, field) in Field::ORDER.iter().enumerate() {
        draw_form_field(frame, form, *field, rows[index]);
    }
    let footer = match &form.problem {
        Some(problem) => Span::styled(problem.clone(), Style::default().fg(Color::Red)),
        None => Span::styled("Tab next field · ←/→ choose · Enter launches · Alt+Enter newline · Esc cancels", MUTED),
    };
    frame.render_widget(Paragraph::new(footer), rows[6]);
}

fn draw_form_field(frame: &mut Frame<'_>, form: &LaunchForm, field: Field, area: Rect) {
    let active = form.field == field;
    let label_style = if active { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { MUTED };
    frame.render_widget(Paragraph::new(Span::styled(format!("{:<10}", field.label()), label_style)), Rect { width: area.width.min(10), ..area });
    let value = Rect { x: area.x + 10.min(area.width), width: area.width.saturating_sub(10), ..area };
    match field {
        Field::Provider => frame.render_widget(Paragraph::new(choice(form.provider.label(), active)), value),
        Field::Account => {
            let label = form.chosen_account().map_or_else(|| format!("no {} account", form.provider.label()), |account| format!("{} ({} of {})", account.label, form.account + 1, form.provider_accounts().len()));
            frame.render_widget(Paragraph::new(choice(&label, active)), value);
        }
        _ => {
            if let Some(area) = form.text_area(field) {
                frame.render_widget(area, value);
            }
        }
    }
}

fn choice(label: &str, active: bool) -> Line<'static> {
    let style = if active { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
    Line::from(vec![Span::styled("‹ ", MUTED), Span::styled(label.to_owned(), style), Span::styled(" ›", MUTED)])
}
