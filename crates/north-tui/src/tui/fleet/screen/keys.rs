//! Key routing: the help panel, the control table and each focus's own handler turn one key event into a command.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::fleet::gates::Decision;
use crate::fleet::model::{Delivery, Status};

use super::super::editor::edit_textarea;
use super::super::form::{FormOutcome, newline_modifier};
use super::super::help_panel::{Reaction, Tab};
use super::super::key_table::{Control, control_for};
use super::pane::option_number;
use super::{Command, Dialog, Focus, Screen};

impl Screen {
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        if self.dialog.is_some() {
            return self.handle_dialog_key(key);
        }
        match self.help.key(key, &self.composer_text()) {
            Reaction::Swallow => return None,
            Reaction::Close { clear_composer } => {
                if clear_composer {
                    self.composer = super::composer_area();
                }
                return None;
            }
            Reaction::Run(name) => return self.run_slash(name),
            Reaction::Pass => {}
        }
        let command = self.route_key(key);
        self.settle_help();
        command
    }

    fn route_key(&mut self, key: KeyEvent) -> Option<Command> {
        if let Some(action) = control_for(key) {
            return self.handle_control(action);
        }
        self.notice = None;
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => self.toggle_focus(),
            KeyCode::PageUp if self.digest.shown => self.digest.page_by(-1),
            KeyCode::PageDown if self.digest.shown => self.digest.page_by(1),
            KeyCode::PageUp => self.scroll_by(i32::from(self.page)),
            KeyCode::PageDown => self.scroll_by(-i32::from(self.page)),
            _ if self.focus == Focus::List => return self.handle_list_key(key),
            _ => return self.handle_composer_key(key),
        }
        None
    }

    fn composer_text(&self) -> String {
        self.composer.lines().join("\n")
    }

    pub(super) fn settle_help(&mut self) {
        if self.dialog.is_some() {
            self.help.close();
        } else {
            let text = self.composer_text();
            self.help.reconcile(&text);
        }
    }

    fn run_slash(&mut self, name: &str) -> Option<Command> {
        self.composer = super::composer_area();
        if name == "/help" {
            self.composer.insert_str("?");
            self.help.open(Tab::General);
            return None;
        }
        self.composer.insert_str(name);
        let command = self.submit(Delivery::Queued);
        self.settle_help();
        command
    }

    fn handle_control(&mut self, control: Control) -> Option<Command> {
        match control {
            Control::Quit => self.quitting = true,
            Control::NewSession => self.open_launch_form(),
            Control::Interrupt => return self.interrupt_selected(),
            Control::Stop => self.confirm_stop(),
            Control::Forget => return self.forget_selected(),
            Control::Deepen => return self.deepen(),
            Control::Digest => self.toggle_digest(),
            Control::PasteImage => self.attach_image(),
            Control::CopyReply => self.copy_reply(),
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
            KeyCode::Enter if newline_modifier(key) => {
                self.held_answer = None;
                self.composer.insert_newline();
            }
            KeyCode::Enter => return self.submit(Delivery::Queued),
            KeyCode::Esc if self.held_answer.take().is_some() => {}
            KeyCode::Esc => return self.escape(),
            _ => {
                self.held_answer = None;
                edit_textarea(&mut self.composer, key);
            }
        }
        None
    }

    fn escape(&mut self) -> Option<Command> {
        if let Some(session) = self.selected_session().filter(|session| session.status == Status::Working) {
            return Some(Command::Interrupt(session.record.name.clone()));
        }
        self.focus = Focus::List;
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
                FormOutcome::Launch(request) => {
                    self.sent_forms.insert(request.name.clone(), form);
                    Some(Command::Launch(request))
                }
                FormOutcome::Cancelled => None,
                FormOutcome::Editing => {
                    self.dialog = Some(Dialog::Launch(form));
                    None
                }
            },
        }
    }
}
