use crossterm::event::{KeyCode, KeyEvent};
use tui_textarea::TextArea;
use ratatui::{Frame, layout::Rect, style::{Color, Style}, text::Line, widgets::{Block, Borders, Clear, Paragraph}};

const COMMANDS: &[(&str, &str)] = &[
    ("/model", "choose the model for next turns"),
    ("/effort", "choose reasoning effort"),
    ("/goal", "show, set, or clear the standing goal"),
    ("/compact", "compact context"),
    ("/rename", "rename this session"),
    ("/fork", "start a sibling from this transcript"),
    ("/config", "open the Context Switchboard"),
    ("/help", "show keys and commands"),
];

#[derive(Default)]
pub struct SlashMenu {
    selected: usize,
    dismissed: Option<String>,
}

enum SlashKey {
    Held,
    Insert(String),
    Complete(String),
}

impl SlashMenu {
    fn matches(&self, input: &str) -> Vec<(&'static str, &'static str)> {
        if !input.starts_with('/') || input.contains(char::is_whitespace) || self.dismissed.as_deref() == Some(input) {
            return Vec::new();
        }
        COMMANDS.iter().copied().filter(|(name, _)| name.starts_with(input)).collect()
    }

    pub fn intercept(&mut self, composer: &mut TextArea<'static>, key: KeyEvent) -> bool {
        let Some(outcome) = self.handle(&composer.lines().join("\n"), key) else { return false };
        let (text, consumed) = match outcome {
            SlashKey::Held => return true,
            SlashKey::Insert(text) => (text, true),
            SlashKey::Complete(command) => (command, false),
        };
        composer.select_all();
        composer.insert_str(text);
        consumed
    }

    fn handle(&mut self, input: &str, key: KeyEvent) -> Option<SlashKey> {
        let matches = self.matches(input);
        if matches.is_empty() { return None; }
        self.selected = self.selected.min(matches.len() - 1);
        match key.code {
            KeyCode::Up => self.selected = (self.selected + matches.len() - 1) % matches.len(),
            KeyCode::Down => self.selected = (self.selected + 1) % matches.len(),
            KeyCode::Esc => self.dismissed = Some(input.to_owned()),
            KeyCode::Enter | KeyCode::Tab => {
                let name = matches[self.selected].0;
                self.selected = 0;
                self.dismissed = None;
                return Some(if key.code == KeyCode::Enter { SlashKey::Complete(name.to_owned()) } else { SlashKey::Insert(format!("{name} ")) });
            }
            _ => { self.selected = 0; self.dismissed = None; return None; }
        }
        Some(SlashKey::Held)
    }

    pub fn draw(&self, frame: &mut Frame<'_>, composer: Rect, input: &str) {
        let matches = self.matches(input);
        if matches.is_empty() { return; }
        let height = (matches.len() as u16 + 3).min(composer.y);
        if height < 3 { return; }
        let area = Rect::new(composer.x, composer.y - height, composer.width.min(72), height);
        let visible = usize::from(height - 3);
        let selected = self.selected.min(matches.len() - 1);
        let start = selected.saturating_sub(visible.saturating_sub(1));
        let mut lines: Vec<Line<'static>> = matches.iter().enumerate().skip(start).take(visible).map(|(index, (name, description))| {
            Line::styled(format!("{} {name:<12} {description}", if index == selected { "›" } else { " " }), Style::default().fg(if index == selected { Color::Cyan } else { Color::Gray }))
        }).collect();
        lines.push(Line::from("↑/↓ select · Enter send · Tab insert · Esc close"));
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" commands ").style(Style::default().bg(Color::Rgb(24, 26, 31)))), area);
    }
}
