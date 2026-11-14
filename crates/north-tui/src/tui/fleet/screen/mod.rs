//! The fleet window's state: what is selected, what is being typed, and the commands the window asks its desk to perform.

mod actions;
mod clipboard;
mod commands;
mod composer;
mod keys;
mod pane;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use ratatui::style::{Modifier, Style};
use tui_textarea::{TextArea, WrapMode};

use crate::fleet::books::Digest;
use crate::fleet::gates::Decision;
use crate::fleet::model::{Account, Delivery, Effort, LaunchRequest, Role, Status};
use crate::fleet::registry::Choices;

use super::attach::Attachments;
use super::clipboard::Clipboard;
use super::desk::{AccountRoom, SessionSummary};
use super::digest::{DigestPane, SessionFacts};
use super::form::LaunchForm;
use super::help_panel::HelpPanel;
use super::order::arrange;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    List,
    Composer,
}

pub enum Dialog {
    Launch(Box<LaunchForm>),
    ConfirmStop(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    Info(String),
    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Launch(Box<LaunchRequest>),
    Send { name: String, text: String, delivery: Delivery, deepen: bool },
    Revive(String),
    Relaunch { name: String, model: Option<String>, effort: Option<Effort> },
    Interrupt(String),
    Stop(String),
    Forget(String),
    Decide { gate: i64, session: String, decision: Decision },
}

impl Command {
    pub fn session(&self) -> &str {
        match self {
            Command::Launch(request) => &request.name,
            Command::Send { name, .. } | Command::Decide { session: name, .. } | Command::Revive(name) | Command::Relaunch { name, .. } | Command::Interrupt(name) | Command::Stop(name) | Command::Forget(name) => name,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingMessage {
    pub session: String,
    pub text: String,
    echoes_before: usize,
}

pub struct Screen {
    pub sessions: Vec<SessionSummary>,
    pub selected: Option<String>,
    pub focus: Focus,
    focus_before_digest: Focus,
    pub composer: TextArea<'static>,
    pub dialog: Option<Dialog>,
    pub notice: Option<Notice>,
    pub scroll: u16,
    pub unread: BTreeSet<String>,
    pub busy: BTreeMap<String, usize>,
    pub pending: Vec<PendingMessage>,
    launching: BTreeMap<String, Option<String>>,
    sent_forms: BTreeMap<String, Box<LaunchForm>>,
    pub digest: DigestPane,
    pub help: HelpPanel,
    held_answer: Option<(i64, String)>,
    attachments: Attachments,
    clipboard: Clipboard,
    terminal_output: String,
    bell: bool,
    scroll_limit: u16,
    page: u16,
    accounts: Vec<Account>,
    choices: Vec<Choices>,
    rooms: Vec<AccountRoom>,
    cwd: PathBuf,
    quitting: bool,
}

impl Screen {
    pub fn new(cwd: PathBuf, accounts: Vec<Account>) -> Self {
        Self {
            sessions: Vec::new(),
            selected: None,
            focus: Focus::Composer,
            focus_before_digest: Focus::Composer,
            composer: composer_area(),
            dialog: None,
            notice: None,
            scroll: 0,
            unread: BTreeSet::new(),
            busy: BTreeMap::new(),
            pending: Vec::new(),
            launching: BTreeMap::new(),
            sent_forms: BTreeMap::new(),
            digest: DigestPane::default(),
            help: HelpPanel::default(),
            held_answer: None,
            attachments: Attachments::system(),
            clipboard: Clipboard::system(),
            terminal_output: String::new(),
            bell: false,
            scroll_limit: u16::MAX,
            page: 10,
            accounts,
            choices: Vec::new(),
            rooms: Vec::new(),
            cwd,
            quitting: false,
        }
    }

    pub fn offer(&mut self, choices: Vec<Choices>) {
        self.choices = choices;
    }

    pub fn offer_rooms(&mut self, rooms: Vec<AccountRoom>) {
        self.rooms = rooms;
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn workspace(&self) -> &std::path::Path {
        &self.cwd
    }

    pub fn quitting(&self) -> bool {
        self.quitting
    }

    pub fn selected_session(&self) -> Option<&SessionSummary> {
        let name = self.selected.as_deref()?;
        self.sessions.iter().find(|session| session.record.name == name)
    }

    pub fn sync(&mut self, sessions: Vec<SessionSummary>) {
        let sessions = arrange(sessions);
        self.mark_finished(&sessions);
        self.sessions = sessions;
        if self.selected_session().is_none() {
            let home = self.sessions.iter().find(|session| session.record.role() == Role::Proxy).or(self.sessions.first()).map(|session| session.record.name.clone());
            self.select(home);
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn with_clipboard(mut self, clipboard: Clipboard) -> Self {
        self.clipboard = clipboard;
        self
    }

    pub fn take_terminal_output(&mut self) -> String {
        std::mem::take(&mut self.terminal_output)
    }

    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    pub fn observe(&mut self, books: Digest, facts: BTreeMap<String, SessionFacts>) {
        self.digest.facts = facts;
        for session in self.digest.update(books) {
            self.bell = true;
            if self.selected.as_ref() != Some(&session) {
                self.unread.insert(session);
            }
        }
    }

    fn mark_finished(&mut self, sessions: &[SessionSummary]) {
        for session in sessions {
            let name = &session.record.name;
            let was_working = self.sessions.iter().any(|previous| &previous.record.name == name && previous.status == Status::Working);
            if was_working && session.status != Status::Working && self.selected.as_ref() != Some(name) {
                self.bell |= self.unread.insert(name.clone());
            }
        }
        self.unread.retain(|name| sessions.iter().any(|session| &session.record.name == name));
    }

    pub fn measure_viewport(&mut self, rows: u16, limit: u16) {
        self.page = (rows / 2).max(1);
        self.scroll_limit = limit;
        self.scroll = self.scroll.min(limit);
    }

    pub fn fail(&mut self, error: &crate::error::NorthError) {
        self.notice = Some(Notice::Error(error.user_message()));
    }
}

fn composer_area() -> TextArea<'static> {
    let mut area = TextArea::default();
    area.set_wrap_mode(WrapMode::WordOrGlyph);
    area.set_max_rows(8);
    area.set_cursor_line_style(Style::default());
    area.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    area
}
