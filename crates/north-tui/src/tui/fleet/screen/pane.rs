//! The panes: which one has focus, which session or gate is selected, and how the viewport scrolls.

use crate::fleet::gates::Decision;

use super::{Command, Focus, Notice, Screen};

impl Screen {
    pub(super) fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::List => Focus::Composer,
            Focus::Composer => Focus::List,
        };
    }

    pub(super) fn toggle_digest(&mut self) {
        self.digest.shown = !self.digest.shown;
        if self.digest.shown {
            self.focus_before_digest = self.focus;
            self.focus = Focus::List;
        } else {
            self.focus = self.focus_before_digest;
        }
    }

    pub(super) fn decide(&mut self, decision: Decision) -> Option<Command> {
        let Some(gate) = self.digest.selected() else {
            self.notice = Some(Notice::Info("no open gate".into()));
            return None;
        };
        if let Decision::Option(number) = decision {
            gate.option(number)?;
        }
        Some(Command::Decide { gate: gate.id, session: gate.request.session.clone(), decision })
    }

    pub(super) fn scroll_by(&mut self, delta: i32) {
        let target = (i32::from(self.scroll) + delta).clamp(0, i32::from(self.scroll_limit));
        self.scroll = u16::try_from(target).unwrap_or(0);
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.sessions.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_deref()
            .and_then(|name| self.sessions.iter().position(|session| session.record.name == name))
            .unwrap_or(0);
        let next = current.saturating_add_signed(delta).min(self.sessions.len() - 1);
        self.select(Some(self.sessions[next].record.name.clone()));
    }

    pub(super) fn select(&mut self, name: Option<String>) {
        if let Some(name) = &name {
            self.unread.remove(name);
        }
        if self.selected != name {
            self.selected = name;
            self.scroll = 0;
            self.scroll_limit = u16::MAX;
        }
    }
}

pub(super) fn option_number(digit: char) -> usize {
    digit.to_digit(10).and_then(|number| usize::try_from(number).ok()).unwrap_or(0)
}
