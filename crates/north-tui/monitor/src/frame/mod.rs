mod builds;
mod fleet;
mod health;
mod paint;
mod queue;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::budget::{Budgets, Tone};
use crate::snapshot::Snapshot;
use crate::text::clock;
use paint::{Boxed, Painter, dim, style};

pub struct View<'a> {
    pub snapshot: &'a Snapshot,
    pub budgets: &'a Budgets,
    pub offset_s: i32,
    pub mode: &'a str,
}

fn badge(text: String, foreground: Color, background: Color) -> Span<'static> {
    Span::styled(text, Style::new().fg(foreground).bg(background).add_modifier(Modifier::BOLD))
}

fn joined(alerts: &[(Tone, String)], tone: Tone) -> Vec<String> {
    alerts.iter().filter(|(alert, _)| *alert == tone).map(|(_, text)| text.clone()).collect()
}

fn status_line(view: &View, alerts: &[(Tone, String)]) -> Line<'static> {
    let level = view.snapshot.ladder.value.as_ref().map_or_else(|| " L? ".to_owned(), |ladder| format!(" L{} ", ladder.level));
    let reds = joined(alerts, Tone::Red);
    let ambers = joined(alerts, Tone::Amber);
    let mut spans = vec![badge(level, Color::Black, Color::White), Span::styled(format!(" {} {} ", view.mode, clock(view.snapshot.at, view.offset_s)), dim())];
    if reds.is_empty() {
        spans.push(badge(" NOTHING RED ".into(), Color::Black, Color::Green));
    } else {
        spans.push(badge(format!(" RED {} ", reds.len()), Color::White, Color::Red));
        spans.push(Span::styled(format!(" {} ", reds.join(" · ")), style(Tone::Red)));
    }
    if !ambers.is_empty() {
        spans.push(badge(format!(" AMBER {} ", ambers.len()), Color::Black, Color::Yellow));
        spans.push(Span::styled(format!(" {}", ambers.join(" · ")), style(Tone::Amber)));
    }
    Line::from(spans)
}

fn section(frame: &mut Frame, area: Rect, (title, lines): Boxed) {
    let block = Block::bordered().title(title).border_style(dim());
    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

pub fn draw(frame: &mut Frame, view: &View) {
    let mut painter = Painter { budgets: view.budgets, now: view.snapshot.at, alerts: Vec::new() };
    let fleet = fleet::render(&mut painter, view.snapshot);
    let queue = queue::render(&mut painter, view.snapshot);
    let builds = builds::render(&mut painter, view.snapshot);
    let health = health::render(&mut painter, view.snapshot, view.offset_s);
    let [status, body] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(frame.area());
    frame.render_widget(Paragraph::new(status_line(view, &painter.alerts)).wrap(Wrap { trim: false }), status);
    let [top, bottom] = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [top_left, top_right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(top);
    let [bottom_left, bottom_right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(bottom);
    section(frame, top_left, fleet);
    section(frame, top_right, queue);
    section(frame, bottom_left, health);
    section(frame, bottom_right, builds);
}

#[cfg(test)]
mod tests;
