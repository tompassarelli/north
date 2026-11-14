//! Paste and clipboard: text or an image goes into the composer, and the selected session's last reply goes out.

use super::super::clipboard::osc52_copy;
use super::{Dialog, Focus, Notice, Screen};

impl Screen {
    pub fn paste(&mut self, text: &str) {
        self.insert_pasted(&text.replace("\r\n", "\n").replace('\r', "\n"));
        self.settle_help();
    }

    fn insert_pasted(&mut self, text: &str) {
        match &mut self.dialog {
            Some(Dialog::Launch(form)) => form.paste(text),
            Some(Dialog::ConfirmStop(_)) => {}
            None if text.is_empty() => self.attach_image(),
            None => {
                self.focus = Focus::Composer;
                self.composer.insert_str(text);
            }
        }
    }

    pub(super) fn attach_image(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        match self.attachments.attach(&self.clipboard) {
            Ok(chip) => {
                self.focus = Focus::Composer;
                self.composer.insert_str(chip);
            }
            Err(error) => self.fail(&error),
        }
    }

    pub(super) fn copy_reply(&mut self) {
        let Some(name) = self.selected.clone() else { return };
        let Some(reply) = self.digest.facts.get(&name).and_then(|facts| facts.reply.clone()) else {
            self.notice = Some(Notice::Error(format!("{name} has no finished reply to copy")));
            return;
        };
        let route = match self.clipboard.put_text(&reply) {
            Ok(()) => "to the desktop clipboard",
            Err(_) => {
                self.terminal_output.push_str(&osc52_copy(&reply));
                "through the terminal"
            }
        };
        self.notice = Some(Notice::Info(format!("copied {name}'s last reply ({} words) {route}", reply.split_whitespace().count())));
    }
}
