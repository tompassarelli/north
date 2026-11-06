use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Role, SessionRecord, Status};
use crate::fleet::prompt::PromptMode;
use crate::fleet::records::write_record;
use crate::fleet::runtime::{PROXY, Runtime, WindowInbox, now_ms};
use crate::fleet::store::{Claim, SessionRow};

use super::takeover::Predecessor;
use super::{Fleet, hand_in};

impl Fleet {
    pub fn adopt(&self, name: &str, conversation: &str, cwd: PathBuf) -> NorthResult<SessionRecord> {
        let (runtime, inbox) = self.window_of(conversation).ok_or_else(|| NorthError::State(format!("no live interactive window holds conversation {conversation}")))?;
        let session_dir = self.session_dir(name)?;
        if let Ok(held) = self.record(name)
            && held.adopted()
            && held.conversation.as_deref() == Some(conversation)
        {
            return Ok(held);
        }
        let role = if name == PROXY { Role::Proxy } else { Role::Lead };
        let request = LaunchRequest {
            verification: false,
            name: name.to_owned(),
            provider: runtime.provider(),
            account: inbox.account.clone(),
            cwd,
            model: None,
            effort: None,
            prompt: String::new(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::default(),
            idle_exit_minutes: None,
            parent: None,
            task_ref: None,
            placement: Some(format!("adopted window, process {}", inbox.pid)),
            machine: None,
            capacity_need: None,
            role: Some(role),
            resume: None,
            compact_window: None,
        };
        let mut record = SessionRecord::launched(&request, None, Some(conversation.to_owned()), false, now_ms());
        record.adopted_ms = Some(record.created_ms);
        let row = SessionRow { status: Status::Idle.label().to_owned(), ..self.table_row(&record, record.created_ms) };
        self.registrar.register(&row, Claim::TakeOver)?;
        let predecessor = self.retire_for(name, Claim::TakeOver)?;
        record.addressed_since_ms = predecessor.as_ref().map(Predecessor::addressed_since);
        fs::create_dir_all(&session_dir)?;
        if let Some(predecessor) = &predecessor {
            hand_in::inherit_journal(&predecessor.archived, &session_dir)?;
        }
        write_record(&session_dir, &record)?;
        Ok(record)
    }

    pub fn caller_conversation(&self) -> Option<String> {
        self.runtimes.caller_conversation()
    }

    pub(super) fn window_of(&self, conversation: &str) -> Option<(Arc<dyn Runtime>, WindowInbox)> {
        let accounts = self.accounts();
        self.providers().into_iter().filter_map(|provider| self.runtimes.get(&provider).ok()).find_map(|runtime| {
            let own: Vec<_> = accounts.iter().filter(|account| account.provider == runtime.provider()).cloned().collect();
            runtime.window_inbox(&own, conversation).map(|inbox| (runtime, inbox))
        })
    }

    pub(super) fn window_status(&self, record: &SessionRecord) -> Status {
        match record.conversation.as_deref().and_then(|conversation| self.window_of(conversation)) {
            Some(_) if !record.stopped => Status::Idle,
            _ => Status::Exited,
        }
    }

    pub(super) fn tell_window(&self, record: &SessionRecord, text: &str) -> NorthResult<()> {
        let gone = || NorthError::State(format!("{} is adopted by an interactive window that is no longer running; run north fleet adopt --name {} from the window that should hold it", record.name, record.name));
        if record.stopped {
            return Err(gone());
        }
        let (runtime, inbox) = record.conversation.as_deref().and_then(|conversation| self.window_of(conversation)).ok_or_else(gone)?;
        runtime.tell_window(&inbox, text)
    }
}
