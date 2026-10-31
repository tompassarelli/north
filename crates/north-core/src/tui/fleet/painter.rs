use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::fleet::model::{Entry, EntryKind, Transcript};

use super::table::{self, Block};

const OPERATOR: Style = Style::new().fg(Color::Rgb(226, 220, 199)).bg(Color::Rgb(52, 58, 62));
const PENDING: Style = OPERATOR.add_modifier(Modifier::DIM);

#[derive(Default)]
pub struct TranscriptPainter {
    key: Option<PaintKey>,
    lines: Vec<Line<'static>>,
}

#[derive(Clone, Eq, PartialEq)]
struct PaintKey {
    name: String,
    entries: usize,
    tail: usize,
    pending: usize,
    width: u16,
}

impl TranscriptPainter {
    pub fn lines(&mut self, name: &str, transcript: Option<&Transcript>, pending: &[&str], width: u16) -> &[Line<'static>] {
        let entries = transcript.map_or(&[][..], |transcript| transcript.entries.as_slice());
        let key = PaintKey {
            name: name.to_owned(),
            entries: entries.len(),
            tail: entries.last().map_or(0, |entry| entry.text.len()),
            pending: pending.len(),
            width,
        };
        if self.key.as_ref() != Some(&key) {
            self.lines = paint(entries, pending, usize::from(width.max(1)));
            self.key = Some(key);
        }
        &self.lines
    }
}

pub fn paint(entries: &[Entry], pending: &[&str], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut previous: Option<EntryKind> = None;
    for entry in entries {
        if entry.kind == EntryKind::TurnEnd {
            previous = Some(EntryKind::TurnEnd);
            continue;
        }
        if previous.is_some() && !(is_tool(entry.kind) && previous.is_some_and(is_tool)) {
            lines.push(Line::default());
        }
        paint_entry(entry, width, &mut lines);
        previous = Some(entry.kind);
    }
    for text in pending {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        paint_user(text, width, PENDING, &mut lines);
    }
    lines
}

fn is_tool(kind: EntryKind) -> bool {
    matches!(kind, EntryKind::Tool | EntryKind::ToolResult)
}

fn paint_entry(entry: &Entry, width: usize, lines: &mut Vec<Line<'static>>) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    match entry.kind {
        EntryKind::User => paint_user(&entry.text, width, OPERATOR, lines),
        EntryKind::Assistant => paint_markdown(&entry.text, width, lines),
        EntryKind::Tool => lines.push(Line::from(vec![Span::styled("  ↳ ", dim), Span::styled(one_line(&entry.text, width.saturating_sub(4)), dim)])),
        EntryKind::ToolResult => lines.push(Line::from(Span::styled(format!("    {}", one_line(&entry.text, width.saturating_sub(4))), dim))),
        EntryKind::Error => lines.extend(prefixed(&entry.text, Style::default().fg(Color::Red))),
        EntryKind::Notice => lines.extend(prefixed(&entry.text, Style::default().fg(Color::Yellow).add_modifier(Modifier::DIM))),
        EntryKind::TurnEnd => {}
    }
}

fn paint_user(text: &str, width: usize, style: Style, lines: &mut Vec<Line<'static>>) {
    for (index, line) in text.lines().enumerate() {
        let marker = if index == 0 { "› " } else { "  " };
        let content = format!("{marker}{line:<padding$}", padding = width.saturating_sub(2));
        lines.push(Line::from(content).style(style));
    }
}

fn paint_markdown(text: &str, width: usize, lines: &mut Vec<Line<'static>>) {
    let mut started = false;
    for line in markdown_lines(text, width.saturating_sub(2)) {
        let blank = line.spans.iter().all(|span| span.content.trim().is_empty());
        if !started && blank {
            continue;
        }
        let prefix = if started { Span::raw("  ") } else { Span::styled("• ", Style::default().fg(Color::Gray)) };
        started = true;
        let mut spans = vec![prefix];
        spans.extend(line.spans);
        lines.push(Line::from(spans));
    }
}

fn markdown_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in table::blocks(text) {
        match block {
            Block::Table(table) => lines.extend(table.lines(width)),
            Block::Prose(prose) => {
                let source = prose.join("
");
                let markdown = tui_markdown::from_str(&source);
                lines.extend(markdown.lines.into_iter().map(|line| {
                    let spans: Vec<Span<'static>> = line.spans.into_iter().map(|span| Span::styled(span.content.into_owned(), line.style.patch(span.style))).collect();
                    Line::from(spans)
                }));
            }
        }
    }
    lines
}

fn prefixed(text: &str, style: Style) -> Vec<Line<'static>> {
    text.lines()
        .enumerate()
        .map(|(index, line)| Line::from(Span::styled(format!("{}{line}", if index == 0 { "• " } else { "  " }), style)))
        .collect()
}

pub fn one_line(text: &str, width: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= width.max(1) {
        return flat;
    }
    let mut cut: String = flat.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
