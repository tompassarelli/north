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

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.tab.is_some()
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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    struct Session {
        panel: HelpPanel,
        text: String,
    }

    impl Session {
        fn new() -> Self {
            Self { panel: HelpPanel::default(), text: String::new() }
        }

        fn press(&mut self, key: KeyEvent) -> Reaction {
            let reaction = self.panel.key(key, &self.text);
            match reaction {
                Reaction::Pass => match key.code {
                    KeyCode::Char(character) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => self.text.push(character),
                    KeyCode::Backspace => {
                        self.text.pop();
                    }
                    KeyCode::Enter => self.text.clear(),
                    _ => {}
                },
                Reaction::Close { clear_composer: true } | Reaction::Run(_) => self.text.clear(),
                Reaction::Close { clear_composer: false } | Reaction::Swallow => {}
            }
            self.panel.reconcile(&self.text);
            reaction
        }
    }

    fn arbitrary_key() -> impl Strategy<Value = KeyEvent> {
        let codes = prop_oneof![
            Just(KeyCode::Esc),
            Just(KeyCode::Tab),
            Just(KeyCode::BackTab),
            Just(KeyCode::Left),
            Just(KeyCode::Right),
            Just(KeyCode::Up),
            Just(KeyCode::Down),
            Just(KeyCode::Enter),
            Just(KeyCode::Backspace),
            Just(KeyCode::Char('?')),
            Just(KeyCode::Char('/')),
            Just(KeyCode::Char('h')),
            Just(KeyCode::Char('l')),
            Just(KeyCode::Char(' ')),
            proptest::char::range('a', 'z').prop_map(KeyCode::Char),
        ];
        let modifiers = prop_oneof![Just(KeyModifiers::NONE), Just(KeyModifiers::NONE), Just(KeyModifiers::CONTROL), Just(KeyModifiers::ALT), Just(KeyModifiers::SHIFT)];
        (codes, modifiers).prop_map(|(code, modifiers)| KeyEvent::new(code, modifiers))
    }

    fn open_on(tab: Tab) -> HelpPanel {
        let mut panel = HelpPanel::default();
        panel.open(tab);
        panel
    }

    proptest! {
        #[test]
        fn any_key_sequence_leaves_one_view_and_a_valid_composer(keys in proptest::collection::vec(arbitrary_key(), 0..60)) {
            let mut session = Session::new();
            for key in keys {
                session.press(key);
                if session.panel.is_open() {
                    prop_assert!(fits(&session.text), "open over {:?}", session.text);
                    let length = match session.panel.tab() {
                        Some(Tab::Commands) => session.panel.filtered_commands().len(),
                        Some(Tab::Shortcuts) => BINDINGS.len(),
                        _ => 1,
                    };
                    prop_assert!(session.panel.cursor() < length.max(1));
                }
            }
        }

        #[test]
        fn escape_closes_from_any_state(keys in proptest::collection::vec(arbitrary_key(), 0..40)) {
            let mut session = Session::new();
            for key in keys {
                session.press(key);
            }
            session.press(press(KeyCode::Esc, KeyModifiers::NONE));
            prop_assert!(!session.panel.is_open());
        }

        #[test]
        fn each_previous_key_undoes_its_next_key_from_any_tab(start in 0usize..3, pair in 0usize..3) {
            let (next, previous) = [
                (press(KeyCode::Char('l'), KeyModifiers::CONTROL), press(KeyCode::Char('h'), KeyModifiers::CONTROL)),
                (press(KeyCode::Tab, KeyModifiers::NONE), press(KeyCode::BackTab, KeyModifiers::SHIFT)),
                (press(KeyCode::Right, KeyModifiers::NONE), press(KeyCode::Left, KeyModifiers::NONE)),
            ][pair];
            let mut panel = open_on(Tab::ALL[start]);
            panel.key(next, "?");
            prop_assert_eq!(panel.tab(), Some(Tab::ALL[(start + 1) % 3]));
            panel.key(previous, "?");
            prop_assert_eq!(panel.tab(), Some(Tab::ALL[start]));
        }

        #[test]
        fn three_steps_either_way_wrap_back_to_the_same_tab(start in 0usize..3) {
            for code in [KeyCode::Right, KeyCode::Left] {
                let mut panel = open_on(Tab::ALL[start]);
                for _ in 0..3 {
                    panel.key(press(code, KeyModifiers::NONE), "?");
                }
                prop_assert_eq!(panel.tab(), Some(Tab::ALL[start]));
            }
        }
    }

    #[test]
    fn question_mark_in_an_empty_composer_opens_general_and_slash_opens_commands() {
        let mut panel = HelpPanel::default();
        assert_eq!(panel.key(press(KeyCode::Char('?'), KeyModifiers::SHIFT), ""), Reaction::Pass);
        assert_eq!(panel.tab(), Some(Tab::General));
        let mut panel = HelpPanel::default();
        panel.key(press(KeyCode::Char('/'), KeyModifiers::NONE), "");
        assert_eq!(panel.tab(), Some(Tab::Commands));
    }

    #[test]
    fn question_mark_inside_a_message_stays_closed_and_types() {
        let mut panel = HelpPanel::default();
        assert_eq!(panel.key(press(KeyCode::Char('?'), KeyModifiers::NONE), "why"), Reaction::Pass);
        assert!(!panel.is_open());
    }

    #[test]
    fn escape_clears_a_lone_trigger_but_keeps_a_longer_filter() {
        let mut panel = open_on(Tab::General);
        assert_eq!(panel.key(press(KeyCode::Esc, KeyModifiers::NONE), "?"), Reaction::Close { clear_composer: true });
        let mut panel = open_on(Tab::Commands);
        assert_eq!(panel.key(press(KeyCode::Esc, KeyModifiers::NONE), "/mo"), Reaction::Close { clear_composer: false });
    }

    #[test]
    fn enter_runs_the_highlighted_filtered_command() {
        let mut session = Session::new();
        for character in "/e".chars() {
            session.press(press(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(session.panel.filtered_commands().first().map(|(name, _)| *name), Some("/effort"));
        assert_eq!(session.press(press(KeyCode::Enter, KeyModifiers::NONE)), Reaction::Run("/effort"));
        assert!(!session.panel.is_open() && session.text.is_empty());
    }

    #[test]
    fn backspace_edits_the_filter_but_otherwise_goes_to_the_previous_tab() {
        let mut session = Session::new();
        for character in "/mo".chars() {
            session.press(press(KeyCode::Char(character), KeyModifiers::NONE));
        }
        session.press(press(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!((session.text.as_str(), session.panel.tab()), ("/m", Some(Tab::Commands)));
        session.press(press(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!((session.text.as_str(), session.panel.tab()), ("/", Some(Tab::Commands)));
        session.press(press(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(session.panel.tab(), Some(Tab::General));
    }

    #[test]
    fn closed_backspace_and_ctrl_h_pass_through_to_the_composer() {
        let mut panel = HelpPanel::default();
        assert_eq!(panel.key(press(KeyCode::Backspace, KeyModifiers::NONE), "ab"), Reaction::Pass);
        assert_eq!(panel.key(press(KeyCode::Char('h'), KeyModifiers::CONTROL), "ab"), Reaction::Pass);
        assert!(!panel.is_open());
    }

    #[test]
    fn typing_after_a_lone_question_mark_closes_the_panel() {
        let mut session = Session::new();
        session.press(press(KeyCode::Char('?'), KeyModifiers::NONE));
        session.press(press(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!session.panel.is_open());
        assert_eq!(session.text, "?a");
    }
}
