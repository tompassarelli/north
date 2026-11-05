use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const GUTTER: &str = " │ ";
const MIN_COLUMN: usize = 3;

pub struct Table {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

pub enum Block<'a> {
    Prose(Vec<&'a str>),
    Table(Table),
}

pub fn blocks(text: &str) -> Vec<Block<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut prose = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if starts_table(&lines, at) {
            if !prose.is_empty() {
                blocks.push(Block::Prose(std::mem::take(&mut prose)));
            }
            let end = (at + 2..lines.len()).find(|&index| !lines[index].contains('|')).unwrap_or(lines.len());
            blocks.push(Block::Table(Table { header: cells(lines[at]), rows: lines[at + 2..end].iter().map(|line| cells(line)).collect() }));
            at = end;
        } else {
            prose.push(lines[at]);
            at += 1;
        }
    }
    if !prose.is_empty() {
        blocks.push(Block::Prose(prose));
    }
    blocks
}

fn starts_table(lines: &[&str], at: usize) -> bool {
    lines[at].contains('|') && lines.get(at + 1).is_some_and(|line| is_rule(line)) && cells(lines[at]).len() == cells(lines[at + 1]).len()
}

fn is_rule(line: &str) -> bool {
    let cells = cells(line);
    !cells.is_empty() && cells.iter().all(|cell| cell.contains('-') && cell.chars().all(|c| matches!(c, '-' | ':')))
}

fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = if line.ends_with("\\|") { line } else { line.strip_suffix('|').unwrap_or(line) };
    let mut cells = vec![String::new()];
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        let cell = cells.last_mut().expect("cells starts non-empty");
        match character {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(String::new()),
            other => cell.push(other),
        }
    }
    cells.into_iter().map(|cell| cell.trim().replace('`', "")).collect()
}

impl Table {
    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let widths = fit(&self.natural(), width);
        if widths.iter().sum::<usize>() + GUTTER.chars().count() * widths.len().saturating_sub(1) > width {
            return self.plain();
        }
        let mut lines = row_lines(&self.header, &widths, Style::default().add_modifier(Modifier::BOLD));
        lines.push(Line::from(Span::styled(rule(&widths), Style::default().fg(Color::DarkGray))));
        for row in &self.rows {
            lines.extend(row_lines(row, &widths, Style::default()));
        }
        lines
    }

    fn natural(&self) -> Vec<usize> {
        (0..self.header.len())
            .map(|column| std::iter::once(&self.header).chain(&self.rows).map(|row| row.get(column).map_or(0, |cell| cell.chars().count())).max().unwrap_or(0).max(1))
            .collect()
    }

    fn plain(&self) -> Vec<Line<'static>> {
        std::iter::once(&self.header).chain(&self.rows).map(|row| Line::from(row.join(" · "))).collect()
    }
}

fn fit(natural: &[usize], width: usize) -> Vec<usize> {
    let gutters = GUTTER.chars().count() * natural.len().saturating_sub(1);
    let budget = width.saturating_sub(gutters);
    let mut widths = natural.to_vec();
    while widths.iter().sum::<usize>() > budget {
        let Some((widest, size)) = widths.iter().copied().enumerate().max_by_key(|&(index, size)| (size, std::cmp::Reverse(index))) else { break };
        if size <= MIN_COLUMN {
            break;
        }
        widths[widest] = size - 1;
    }
    widths
}

fn rule(widths: &[usize]) -> String {
    widths.iter().map(|width| "─".repeat(*width)).collect::<Vec<_>>().join("─┼─")
}

fn row_lines(row: &[String], widths: &[usize], style: Style) -> Vec<Line<'static>> {
    let wrapped: Vec<Vec<String>> = widths.iter().enumerate().map(|(column, width)| wrap(row.get(column).map_or("", String::as_str), *width)).collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|line| {
            let mut spans = Vec::new();
            for (column, width) in widths.iter().enumerate() {
                if column > 0 {
                    spans.push(Span::styled(GUTTER, Style::default().fg(Color::DarkGray)));
                }
                let text = wrapped[column].get(line).map_or("", String::as_str);
                spans.push(Span::styled(format!("{text:<width$}"), style));
            }
            Line::from(spans)
        })
        .collect()
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for mut word in text.split_whitespace() {
        while word.chars().count() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let cut = word.char_indices().nth(width).map_or(word.len(), |(index, _)| index);
            lines.push(word[..cut].to_owned());
            word = &word[cut..];
        }
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}
