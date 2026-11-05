use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::key_table::BINDINGS;
use super::slash;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    General,
    Commands,
    Shortcuts,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::General, Tab::Commands, Tab::Shortcuts];

    pub fn label(self) -> &'static str {
        match self {
            Tab::General => "general",
            Tab::Commands => "commands",
            Tab::Shortcuts => "shortcuts",
        }
    }

    fn step(self, forward: bool) -> Tab {
        let count = Self::ALL.len();
        let index = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Self::ALL[if forward { (index + 1) % count } else { (index + count - 1) % count }]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reaction {
    Pass,
    Swallow,
    Close { clear_composer: bool },
    Run(&'static str),
}

#[derive(Debug, Default)]
pub struct HelpPanel {
    tab: Option<Tab>,
    commands_cursor: usize,
    shortcuts_cursor: usize,
    filter: String,
}

pub fn fits(text: &str) -> bool {
    text == "?" || (text.starts_with('/') && !text.contains(char::is_whitespace))
}

impl HelpPanel {
    pub fn tab(&self) -> Option<Tab> {
        self.tab
    }

    pub fn open(&mut self, tab: Tab) {
        self.tab = Some(tab);
    }

    pub fn close(&mut self) {
        self.tab = None;
        self.commands_cursor = 0;
        self.filter.clear();
    }

    pub fn filtered_commands(&self) -> Vec<(&'static str, &'static str)> {
        slash::matching(&self.filter)
    }

    pub fn cursor(&self) -> usize {
        match self.tab {
            Some(Tab::Commands) => self.commands_cursor,
            Some(Tab::Shortcuts) => self.shortcuts_cursor,
            _ => 0,
        }
    }

    pub fn key(&mut self, key: KeyEvent, composer: &str) -> Reaction {
        let Some(tab) = self.tab else { return self.opening(key, composer) };
        let plain = !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.close();
                Reaction::Close { clear_composer: matches!(composer, "?" | "/") }
            }
            KeyCode::Right | KeyCode::Tab if plain => self.switch(tab, true),
            KeyCode::Left | KeyCode::BackTab => self.switch(tab, false),
            KeyCode::Char('l' | 'L') if control => self.switch(tab, true),
            KeyCode::Char('h' | 'H') if control => self.switch(tab, false),
            KeyCode::Backspace if !self.editing_filter(tab, composer) => self.switch(tab, false),
            KeyCode::Up if plain => self.walk(tab, false),
            KeyCode::Down if plain => self.walk(tab, true),
            KeyCode::Enter if plain => self.confirm(tab),
            _ => Reaction::Pass,
        }
    }

    pub fn reconcile(&mut self, composer: &str) {
        if self.tab.is_none() {
            return;
        }
        if !fits(composer) {
            self.close();
            return;
        }
        let filter = if composer.starts_with('/') { composer } else { "" };
        if self.filter != filter {
            self.filter = filter.to_owned();
            self.commands_cursor = 0;
        }
    }

    fn opening(&mut self, key: KeyEvent, composer: &str) -> Reaction {
        let plain = !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if plain && composer.is_empty() {
            match key.code {
                KeyCode::Char('?') => self.open(Tab::General),
                KeyCode::Char('/') => self.open(Tab::Commands),
                _ => {}
            }
        }
        Reaction::Pass
    }

    fn editing_filter(&self, tab: Tab, composer: &str) -> bool {
        tab == Tab::Commands && composer.starts_with('/') && composer.chars().count() > 1
    }

    fn switch(&mut self, tab: Tab, forward: bool) -> Reaction {
        self.tab = Some(tab.step(forward));
        Reaction::Swallow
    }

    fn walk(&mut self, tab: Tab, forward: bool) -> Reaction {
        let (cursor, length) = match tab {
            Tab::General => return Reaction::Swallow,
            Tab::Commands => (&mut self.commands_cursor, slash::matching(&self.filter).len()),
            Tab::Shortcuts => (&mut self.shortcuts_cursor, BINDINGS.len()),
        };
        if length > 0 {
            *cursor = if forward { (*cursor + 1) % length } else { (*cursor + length - 1) % length };
        }
        Reaction::Swallow
    }

    fn confirm(&mut self, tab: Tab) -> Reaction {
        if tab != Tab::Commands {
            return Reaction::Swallow;
        }
        let matches = self.filtered_commands();
        let Some((name, _)) = matches.get(self.commands_cursor.min(matches.len().saturating_sub(1))) else { return Reaction::Pass };
        self.close();
        Reaction::Run(name)
    }
}
