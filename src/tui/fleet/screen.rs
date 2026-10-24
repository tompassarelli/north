use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use tui_textarea::{TextArea, WrapMode};

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::Digest;
use crate::fleet::gates::Decision;
use crate::fleet::model::{Account, Delivery, EntryKind, LaunchRequest, Status, Transcript};
use crate::fleet::runtime::SESSION_ENV;

use super::desk::SessionSummary;
use super::editor::edit_textarea;
use super::digest::{DigestPane, SessionFacts};
use super::form::{FormOutcome, LaunchForm, newline_modifier};
use super::order::arrange;
use super::slash::SlashMenu;

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
    Interrupt(String),
    Stop(String),
    Forget(String),
    Decide { gate: i64, session: String, decision: Decision },
}

impl Command {
    pub fn session(&self) -> &str {
        match self {
            Command::Launch(request) => &request.name,
            Command::Send { name, .. } | Command::Decide { session: name, .. } | Command::Revive(name) | Command::Interrupt(name) | Command::Stop(name) | Command::Forget(name) => name,
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
    pub composer: TextArea<'static>,
    pub dialog: Option<Dialog>,
    pub notice: Option<Notice>,
    pub scroll: u16,
    pub unread: BTreeSet<String>,
    pub busy: BTreeMap<String, usize>,
    pub pending: Vec<PendingMessage>,
    pub digest: DigestPane,
    pub slash: SlashMenu,
    bell: bool,
    scroll_limit: u16,
    page: u16,
    accounts: Vec<Account>,
    cwd: PathBuf,
    quitting: bool,
}

impl Screen {
    pub fn new(cwd: PathBuf, accounts: Vec<Account>) -> Self {
        Self {
            sessions: Vec::new(),
            selected: None,
            focus: Focus::Composer,
            composer: composer_area(),
            dialog: None,
            notice: None,
            scroll: 0,
            unread: BTreeSet::new(),
            busy: BTreeMap::new(),
            pending: Vec::new(),
            digest: DigestPane::default(),
            slash: SlashMenu::default(),
            bell: false,
            scroll_limit: u16::MAX,
            page: 10,
            accounts,
            cwd,
            quitting: false,
        }
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
            let first = self.sessions.first().map(|session| session.record.name.clone());
            self.select(first);
        }
    }

    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
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

    pub fn paste(&mut self, text: &str) {
        match &mut self.dialog {
            Some(Dialog::Launch(form)) => form.paste(text),
            Some(Dialog::ConfirmStop(_)) => {}
            None => {
                self.focus = Focus::Composer;
                self.composer.insert_str(text);
            }
        }
    }

    pub fn fail(&mut self, error: &crate::error::NorthError) {
        self.notice = Some(Notice::Error(error.user_message()));
    }

    pub fn begin(&mut self, command: &Command, transcript: Option<&Transcript>) {
        *self.busy.entry(command.session().to_owned()).or_default() += 1;
        let message = match command {
            Command::Send { name, text, .. } => {
                let waiting = self.pending.iter().filter(|pending| &pending.session == name && &pending.text == text).count();
                self.pending.push(PendingMessage { session: name.clone(), text: text.clone(), echoes_before: echoes(transcript, text) + waiting });
                return;
            }
            Command::Launch(request) => format!("launching {}…", request.name),
            Command::Revive(name) => format!("resuming {name}…"),
            Command::Interrupt(name) => format!("interrupting {name}…"),
            Command::Stop(name) => format!("stopping {name}…"),
            Command::Forget(name) => format!("forgetting {name}…"),
            Command::Decide { gate, .. } => format!("answering gate {gate}…"),
        };
        self.notice = Some(Notice::Info(message));
    }

    pub fn reconcile<'t>(&mut self, transcript_of: impl Fn(&str) -> Option<&'t Transcript>) {
        self.pending.retain(|pending| echoes(transcript_of(&pending.session), &pending.text) <= pending.echoes_before);
    }

    pub fn pending_for(&self, session: &str) -> Vec<&str> {
        self.pending.iter().filter(|pending| pending.session == session).map(|pending| pending.text.as_str()).collect()
    }

    pub fn settle(&mut self, command: Command, outcome: NorthResult<()>) {
        self.release(command.session());
        match (command, outcome) {
            (Command::Launch(request), Ok(())) => {
                self.notice = Some(Notice::Info(format!("launched {}", request.name)));
                self.select(Some(request.name));
                self.focus = Focus::Composer;
            }
            (Command::Send { name, text, .. }, Err(error)) => {
                if let Some(index) = self.pending.iter().rposition(|pending| pending.session == name && pending.text == text) {
                    self.pending.remove(index);
                }
                self.composer.insert_str(text);
                self.fail(&error);
            }
            (Command::Send { .. }, Ok(())) => {}
            (Command::Decide { .. }, Err(error @ NorthError::GateAnswered { .. })) => {
                self.notice = Some(Notice::Info(error.user_message()));
            }
            (Command::Decide { gate, .. }, Ok(())) => self.notice = Some(Notice::Info(format!("answered gate {gate}"))),
            (Command::Decide { decision: Decision::Text(text), .. }, Err(error)) => {
                self.composer.insert_str(text);
                self.fail(&error);
            }
            (Command::Revive(name), Ok(())) => self.notice = Some(Notice::Info(format!("resumed {name}"))),
            (Command::Interrupt(name), Ok(())) => self.notice = Some(Notice::Info(format!("interrupted {name}"))),
            (Command::Stop(name), Ok(())) => self.notice = Some(Notice::Info(format!("stopped {name}"))),
            (Command::Forget(name), Ok(())) => {
                self.notice = Some(Notice::Info(format!("forgot {name}")));
                self.pending.retain(|pending| pending.session != name);
                self.sessions.retain(|session| session.record.name != name);
                let first = self.sessions.first().map(|session| session.record.name.clone());
                self.select(first);
            }
            (_, Err(error)) => self.fail(&error),
        }
    }

    fn release(&mut self, session: &str) {
        if let Some(count) = self.busy.get_mut(session) {
            *count -= 1;
            if *count == 0 {
                self.busy.remove(session);
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        if self.dialog.is_some() {
            return self.handle_dialog_key(key);
        }
        if let Some(action) = control_action(key) {
            return self.handle_control(action);
        }
        if self.focus == Focus::Composer && self.slash.intercept(&mut self.composer, key) {
            return None;
        }
        self.notice = None;
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => self.toggle_focus(),
            KeyCode::PageUp => self.scroll_by(i32::from(self.page)),
            KeyCode::PageDown => self.scroll_by(-i32::from(self.page)),
            _ if self.focus == Focus::List => return self.handle_list_key(key),
            _ => return self.handle_composer_key(key),
        }
        None
    }

    fn handle_control(&mut self, character: char) -> Option<Command> {
        match character {
            'q' => self.quitting = true,
            'n' => self.open_launch_form(),
            'c' => return self.interrupt_selected(),
            'x' => self.confirm_stop(),
            'd' => return self.forget_selected(),
            'g' => return self.deepen(),
            't' => self.toggle_digest(),
            _ => {}
        }
        None
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> Option<Command> {
        if self.digest.shown {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.digest.move_selection(-1),
                KeyCode::Down | KeyCode::Char('j') => self.digest.move_selection(1),
                KeyCode::Char(digit @ '1'..='9') => return self.decide(Decision::Option(option_number(digit))),
                _ => return self.handle_session_key(key),
            }
            return None;
        }
        self.handle_session_key(key)
    }

    fn handle_session_key(&mut self, key: KeyEvent) -> Option<Command> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Home | KeyCode::Char('g') => self.select(self.sessions.first().map(|session| session.record.name.clone())),
            KeyCode::End | KeyCode::Char('G') => self.select(self.sessions.last().map(|session| session.record.name.clone())),
            KeyCode::Enter => self.focus = Focus::Composer,
            KeyCode::Esc => self.scroll = 0,
            KeyCode::Char('r') if self.selected_session().is_some_and(|session| session.status == Status::Exited) => {
                return self.selected.clone().map(Command::Revive);
            }
            KeyCode::Char(_) => {
                self.focus = Focus::Composer;
                edit_textarea(&mut self.composer, key);
            }
            _ => {}
        }
        None
    }

    fn handle_composer_key(&mut self, key: KeyEvent) -> Option<Command> {
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => return self.submit(Delivery::Now),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::ALT) => return self.submit(Delivery::Now),
            KeyCode::Enter if newline_modifier(key) => self.composer.insert_newline(),
            KeyCode::Enter => return self.submit(Delivery::Queued),
            KeyCode::Esc => self.focus = Focus::List,
            _ => edit_textarea(&mut self.composer, key),
        }
        None
    }

    fn handle_dialog_key(&mut self, key: KeyEvent) -> Option<Command> {
        match self.dialog.take()? {
            Dialog::ConfirmStop(name) => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => Some(Command::Stop(name)),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => None,
                _ => {
                    self.dialog = Some(Dialog::ConfirmStop(name));
                    None
                }
            },
            Dialog::Launch(mut form) => match form.handle_key(key) {
                FormOutcome::Launch(request) => Some(Command::Launch(request)),
                FormOutcome::Cancelled => None,
                FormOutcome::Editing => {
                    self.dialog = Some(Dialog::Launch(form));
                    None
                }
            },
        }
    }

    fn deepen(&mut self) -> Option<Command> {
        let mut command = self.submit(Delivery::Queued)?;
        if let Command::Send { deepen, .. } = &mut command {
            *deepen = true;
        }
        Some(command)
    }

    fn toggle_digest(&mut self) {
        self.digest.shown = !self.digest.shown;
        self.focus = if self.digest.shown { Focus::List } else { Focus::Composer };
    }

    fn decide(&mut self, decision: Decision) -> Option<Command> {
        let Some(gate) = self.digest.selected() else {
            self.notice = Some(Notice::Info("no open gate".into()));
            return None;
        };
        if let Decision::Option(number) = decision {
            gate.option(number)?;
        }
        Some(Command::Decide { gate: gate.id, session: gate.request.session.clone(), decision })
    }

    fn submit(&mut self, delivery: Delivery) -> Option<Command> {
        let text = self.composer.lines().join("\n");
        if text.trim().is_empty() {
            return None;
        }
        if self.digest.shown && self.digest.selected().is_some() {
            self.composer = composer_area();
            return self.decide(Decision::Text(text));
        }
        let Some(session) = self.selected_session() else {
            self.notice = Some(Notice::Error("no session selected; Ctrl+N starts one".into()));
            return None;
        };
        let name = session.record.name.clone();
        self.composer = composer_area();
        self.scroll = 0;
        Some(Command::Send { name, text, delivery, deepen: false })
    }

    fn open_launch_form(&mut self) {
        let taken: Vec<String> = self.sessions.iter().map(|session| session.record.name.clone()).collect();
        let parent = env::var(SESSION_ENV).ok();
        self.dialog = Some(Dialog::Launch(Box::new(LaunchForm::new(self.accounts.clone(), &self.cwd, &taken, parent))));
    }

    fn interrupt_selected(&mut self) -> Option<Command> {
        let session = self.selected_session()?;
        if session.status == Status::Working {
            return Some(Command::Interrupt(session.record.name.clone()));
        }
        self.notice = Some(Notice::Info(format!("{} is {}, nothing to interrupt", session.record.name, session.status.label())));
        None
    }

    fn confirm_stop(&mut self) {
        if let Some(session) = self.selected_session() {
            if session.status == Status::Exited {
                self.notice = Some(Notice::Info(format!("{} has already exited; Ctrl+D forgets it", session.record.name)));
            } else {
                self.dialog = Some(Dialog::ConfirmStop(session.record.name.clone()));
            }
        }
    }

    fn forget_selected(&mut self) -> Option<Command> {
        let session = self.selected_session()?;
        if session.status == Status::Exited {
            return Some(Command::Forget(session.record.name.clone()));
        }
        self.notice = Some(Notice::Info(format!("{} is still running; Ctrl+X stops it first", session.record.name)));
        None
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::List => Focus::Composer,
            Focus::Composer => Focus::List,
        };
    }

    fn scroll_by(&mut self, delta: i32) {
        let target = (i32::from(self.scroll) + delta).clamp(0, i32::from(self.scroll_limit));
        self.scroll = u16::try_from(target).unwrap_or(0);
    }

    fn move_selection(&mut self, delta: isize) {
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

    fn select(&mut self, name: Option<String>) {
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

fn composer_area() -> TextArea<'static> {
    let mut area = TextArea::default();
    area.set_wrap_mode(WrapMode::WordOrGlyph);
    area.set_max_rows(8);
    area.set_placeholder_text("message the selected session · Enter sends · Alt+Enter newline");
    area.set_cursor_line_style(Style::default());
    area.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    area
}

fn control_action(key: KeyEvent) -> Option<char> {
    let KeyCode::Char(character) = key.code else { return None };
    let character = character.to_ascii_lowercase();
    (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(character, 'q' | 'n' | 'c' | 'x' | 'd' | 'g' | 't')).then_some(character)
}

fn echoes(transcript: Option<&Transcript>, text: &str) -> usize {
    let text = text.trim();
    transcript.map_or(0, |transcript| transcript.entries.iter().filter(|entry| entry.kind == EntryKind::User && entry.text.trim_end().ends_with(text)).count())
}

fn option_number(digit: char) -> usize {
    digit.to_digit(10).and_then(|number| usize::try_from(number).ok()).unwrap_or(0)
}
