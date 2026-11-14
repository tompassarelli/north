//! Command lifecycle: a command marks its session busy, is reconciled against the transcript's echo, and settles back into a notice.

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::Decision;
use crate::fleet::model::{Effort, EntryKind, Transcript};

use super::super::form::LaunchForm;
use super::{Command, Dialog, Focus, Notice, PendingMessage, Screen};

impl Screen {
    pub fn begin(&mut self, command: &Command, transcript: Option<&Transcript>) {
        *self.busy.entry(command.session().to_owned()).or_default() += 1;
        let message = match command {
            Command::Send { name, text, .. } => {
                let waiting = self.pending.iter().filter(|pending| &pending.session == name && &pending.text == text).count();
                self.pending.push(PendingMessage { session: name.clone(), text: text.clone(), echoes_before: echoes(transcript, text) + waiting });
                return;
            }
            Command::Launch(request) => {
                self.launching.insert(request.name.clone(), self.selected.clone());
                format!("launching {}…", request.name)
            }
            Command::Revive(name) => format!("resuming {name}…"),
            Command::Relaunch { name, .. } => format!("restarting {name}…"),
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
        let (origin, sent_form) = match &command {
            Command::Launch(request) => (self.launching.remove(&request.name), self.sent_forms.remove(&request.name)),
            _ => (None, None),
        };
        match (command, outcome) {
            (Command::Launch(request), Ok(())) => self.launched(request.name, origin),
            (Command::Launch(_), Err(error)) => self.launch_refused(sent_form, &error),
            (Command::Send { name, text, .. }, Err(error)) => self.send_failed(&name, &text, &error),
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
            (Command::Relaunch { name, model, effort }, Ok(())) => self.relaunched(name, model, effort),
            (Command::Interrupt(name), Ok(())) => self.notice = Some(Notice::Info(format!("interrupted {name}"))),
            (Command::Stop(name), Ok(())) => self.notice = Some(Notice::Info(format!("stopped {name}"))),
            (Command::Forget(name), Ok(())) => self.forgotten(name),
            (_, Err(error)) => self.fail(&error),
        }
    }

    fn launched(&mut self, name: String, origin: Option<Option<String>>) {
        if origin.as_ref() == Some(&self.selected) && self.composer.is_empty() {
            self.notice = Some(Notice::Info(format!("launched {name}")));
            self.select(Some(name));
            self.focus = Focus::Composer;
        } else {
            self.notice = Some(Notice::Info(format!("launched {name}; Tab to the session list and select it to write to it")));
        }
    }

    fn launch_refused(&mut self, sent_form: Option<Box<LaunchForm>>, error: &NorthError) {
        match sent_form {
            Some(mut form) if self.dialog.is_none() => {
                form.problem = Some(error.user_message());
                self.dialog = Some(Dialog::Launch(form));
                self.notice = None;
            }
            _ => self.fail(error),
        }
    }

    fn send_failed(&mut self, name: &str, text: &str, error: &NorthError) {
        if let Some(index) = self.pending.iter().rposition(|pending| pending.session == name && pending.text == text) {
            self.pending.remove(index);
        }
        self.composer.insert_str(text);
        self.fail(error);
    }

    fn relaunched(&mut self, name: String, model: Option<String>, effort: Option<Effort>) {
        let changes_model = model.is_some();
        let changes: Vec<String> = model.into_iter().chain(effort.map(|effort| format!("{} effort", effort.label()))).collect();
        let continues = if changes_model { "; on a model of another family the conversation is a new one that starts from the last report" } else { "; the conversation continues" };
        self.notice = Some(Notice::Info(format!("restarted {name} on {}{continues}", changes.join(", "))));
    }

    fn forgotten(&mut self, name: String) {
        self.notice = Some(Notice::Info(format!("forgot {name}")));
        self.pending.retain(|pending| pending.session != name);
        self.sessions.retain(|session| session.record.name != name);
        let first = self.sessions.first().map(|session| session.record.name.clone());
        self.select(first);
    }

    fn release(&mut self, session: &str) {
        if let Some(count) = self.busy.get_mut(session) {
            *count -= 1;
            if *count == 0 {
                self.busy.remove(session);
            }
        }
    }
}

fn echoes(transcript: Option<&Transcript>, text: &str) -> usize {
    let text = text.trim();
    transcript.map_or(0, |transcript| transcript.entries.iter().filter(|entry| entry.kind == EntryKind::User && entry.text.trim_end().ends_with(text)).count())
}
