use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::fleet::books::{Landing, SessionSpend};
use crate::fleet::gates::Gate;
use crate::fleet::model::{Account, Quota};

use super::desk::SessionSummary;
use super::digest::DigestPane;
use super::painter::one_line;
use super::screen::Screen;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);
const KEY: Style = Style::new().fg(Color::Cyan);

pub fn draw_digest(frame: &mut Frame<'_>, screen: &Screen, area: Rect, now_ms: u64) {
    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::Cyan)).title(Span::styled(" digest ", BOLD));
    let width = usize::from(block.inner(area).width);
    frame.render_widget(Paragraph::new(digest_lines(&screen.digest, &screen.sessions, now_ms, width)).block(block).wrap(Wrap { trim: false }), area);
}

pub fn digest_lines(digest: &DigestPane, sessions: &[SessionSummary], now_ms: u64, width: usize) -> Vec<Line<'static>> {
    let open = digest.books.open_gates().count();
    let mut lines = vec![Line::from(vec![Span::styled("decisions", BOLD), Span::styled(format!(" · {open} open"), MUTED)])];
    if digest.books.gates.is_empty() {
        lines.push(Line::from(Span::styled("  no decisions waiting on Tom", MUTED)));
    }
    for gate in &digest.books.gates {
        lines.extend(gate_lines(gate, digest.selected_gate == Some(gate.id), now_ms));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("sessions", BOLD)));
    for session in sessions {
        lines.extend(session_lines(digest, session, width));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("landed today", BOLD)));
    if digest.books.landed.is_empty() {
        lines.push(Line::from(Span::styled("  nothing landed today", MUTED)));
    }
    lines.extend(digest.books.landed.iter().map(landing_line));
    lines
}

fn gate_lines(gate: &Gate, selected: bool, now_ms: u64) -> Vec<Line<'static>> {
    let marker = if selected { Span::styled("› ", KEY) } else { Span::raw("  ") };
    let title = Span::styled(format!("gate {} · {}", gate.id, gate.request.session), BOLD);
    let Some(closure) = &gate.closure else {
        let remaining = gate.default_at_ms().saturating_sub(now_ms);
        let when = if remaining == 0 { "default overdue".to_owned() } else { format!("default in {}", span(remaining)) };
        let mut choices = vec![Span::raw("    ")];
        for (index, option) in gate.request.options.iter().enumerate() {
            let number = index + 1;
            choices.push(Span::styled(format!("{number} "), KEY));
            choices.push(Span::raw(option.clone()));
            let tags: Vec<&str> = [(number == gate.request.recommend, "recommended"), (number == gate.request.default, "default")].into_iter().filter(|(on, _)| *on).map(|(_, tag)| tag).collect();
            if !tags.is_empty() {
                choices.push(Span::styled(format!(" ({})", tags.join(", ")), MUTED));
            }
            choices.push(Span::raw("   "));
        }
        return vec![
            Line::from(vec![marker, title, Span::styled(format!(" · {when}"), Style::default().fg(Color::Yellow))]),
            Line::from(format!("    {}", gate.request.question)),
            Line::from(choices),
        ];
    };
    let verdict = format!(" · {} by {} · {} ago", closure.answer, closure.by.label(), span(now_ms.saturating_sub(closure.closed_ms)));
    vec![Line::from(vec![Span::styled("✓ ", MUTED), Span::styled(format!("gate {} · {}", gate.id, gate.request.session), MUTED), Span::styled(verdict, MUTED)])]
}

fn session_lines(digest: &DigestPane, session: &SessionSummary, width: usize) -> Vec<Line<'static>> {
    let name = &session.record.name;
    let facts = digest.facts.get(name).cloned().unwrap_or_default();
    let status = session.status.label();
    let task = one_line(facts.task.as_deref().unwrap_or("no task yet"), width.saturating_sub(name.chars().count() + status.len() + 6));
    let headline = Line::from(vec![
        Span::styled(format!("  {name} "), BOLD),
        Span::styled(status, super::view::status_style(session.status)),
        Span::styled(format!(" · {task}"), MUTED),
    ]);
    let spend = format!("    {} · ", digest.books.spend_of(name).map_or_else(|| "0 tokens".to_owned(), spend_text));
    let report = one_line(&format!("report: {}", facts.report.as_deref().unwrap_or("no report yet")), width.saturating_sub(spend.chars().count()));
    vec![headline, Line::from(vec![Span::styled(spend, MUTED), Span::raw(report)])]
}

fn spend_text(spend: &SessionSpend) -> String {
    let dollars = spend.api_usd.map_or_else(|| "plan $ unmeasured".to_owned(), |usd| format!("api ${usd:.2}"));
    format!("{} tokens · {dollars}", count(spend.tokens))
}

fn landing_line(landing: &Landing) -> Line<'static> {
    let sha: String = landing.sha.chars().take(9).collect();
    let wall = landing.wall_ms.map_or_else(|| "wall unmeasured".to_owned(), |wall| format!("wall {}", span(wall)));
    Line::from(vec![Span::raw(format!("  {} ", landing.task_ref)), Span::styled(sha, KEY), Span::styled(format!(" · {wall}"), MUTED)])
}

pub fn header_line(accounts: &[Account], quota_of: impl Fn(&Account) -> Option<Quota>) -> Line<'static> {
    let mut spans = Vec::new();
    let mut provider = None;
    for account in accounts {
        if provider != Some(&account.provider) {
            spans.push(Span::styled(format!("{}{} ", if provider.is_some() { "   " } else { " " }, account.provider.label()), BOLD));
            provider = Some(&account.provider);
        } else {
            spans.push(Span::styled(" · ", MUTED));
        }
        spans.push(Span::raw(format!("{} ", account.label)));
        spans.push(match quota_of(account) {
            Some(quota) if !quota.windows.is_empty() => Span::styled(quota_text(&quota), Style::default().fg(Color::Green)),
            _ => Span::styled("unmeasured", MUTED),
        });
    }
    Line::from(spans)
}

fn quota_text(quota: &Quota) -> String {
    let windows: Vec<String> = quota.windows.iter().map(|window| format!("{} {}%", window.label, window.left_percent())).collect();
    format!("{} left", windows.join(" "))
}

pub fn span(millis: u64) -> String {
    let seconds = millis.div_ceil(1000);
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h{:02}m", seconds / 3_600, seconds % 3_600 / 60),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn count(tokens: u64) -> String {
    match tokens {
        0..=9_999 => tokens.to_string(),
        10_000..=999_999 => format!("{:.1}k", tokens as f64 / 1_000.0),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}
