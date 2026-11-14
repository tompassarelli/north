//! Session actions the window offers: start a launch, interrupt, confirm a stop, and forget an exited session.

use std::env;

use crate::fleet::model::Status;
use crate::fleet::runtime::SESSION_ENV;

use super::super::form::LaunchForm;
use super::{Command, Dialog, Notice, Screen};

impl Screen {
    pub(super) fn open_launch_form(&mut self) {
        let taken: Vec<String> = self.sessions.iter().map(|session| session.record.name.clone()).collect();
        let parent = env::var(SESSION_ENV).ok();
        self.dialog = Some(Dialog::Launch(Box::new(LaunchForm::new(self.accounts.clone(), &self.cwd, &taken, parent).with_rooms(&self.rooms))));
    }

    pub(super) fn interrupt_selected(&mut self) -> Option<Command> {
        let session = self.selected_session()?;
        if session.status == Status::Working {
            return Some(Command::Interrupt(session.record.name.clone()));
        }
        self.notice = Some(Notice::Info(format!("{} is {}, nothing to interrupt", session.record.name, session.status.label())));
        None
    }

    pub(super) fn confirm_stop(&mut self) {
        if let Some(session) = self.selected_session() {
            if session.status == Status::Exited {
                self.notice = Some(Notice::Info(format!("{} has already exited; Ctrl+D forgets it", session.record.name)));
            } else {
                self.dialog = Some(Dialog::ConfirmStop(session.record.name.clone()));
            }
        }
    }

    pub(super) fn forget_selected(&mut self) -> Option<Command> {
        let session = self.selected_session()?;
        if session.status == Status::Exited {
            return Some(Command::Forget(session.record.name.clone()));
        }
        self.notice = Some(Notice::Info(format!("{} is still running; Ctrl+X stops it first", session.record.name)));
        None
    }
}
