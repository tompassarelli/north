use std::fs;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Effort, LaunchRequest, Role, SessionRecord, Status};
use crate::fleet::runtime::{Opening, now_ms};
use crate::fleet::store::{Claim, SessionRow};

use super::takeover::Predecessor;
use super::{Fleet, efforts, hand_in, session_running};

impl Fleet {
    pub fn launch(&self, request: &LaunchRequest) -> NorthResult<SessionRecord> {
        self.launch_as(request, Claim::Fresh)
    }

    /// Launches under a name a session may already hold: that session is ended and archived first, and the new one takes every message to the name the old one never took, so two sessions never answer to one name.
    pub fn take_over(&self, request: &LaunchRequest) -> NorthResult<SessionRecord> {
        self.launch_as(request, Claim::TakeOver)
    }

    fn launch_as(&self, request: &LaunchRequest, claim: Claim) -> NorthResult<SessionRecord> {
        let session_dir = self.session_dir(&request.name)?;
        if session_dir.exists() && claim == Claim::Fresh {
            return Err(NorthError::Usage(format!("a session named {} already exists; --take-over ends it and moves the name and its messages to the new session", request.name)));
        }
        if let Some(parent) = &request.parent {
            self.reaches(parent)?;
        }
        self.refuse_second_proxy(request, claim)?;
        let account = self.account(request)?;
        let runtime = self.runtimes.get(&request.provider)?;
        if let Some(model) = request.model.as_deref().filter(|model| !runtime.models().contains(model)) {
            return Err(NorthError::Usage(format!("{} has no model {model}; launch one of {}", request.provider.label(), runtime.models().join(", "))));
        }
        let effort = efforts::taken(runtime.as_ref(), request.model.as_deref(), request.effort)?;
        let conversation = runtime.assign_conversation()?;
        let verification = request.verification || std::env::var_os("NORTH_FLEET_TEST").is_some() || request.parent.as_deref().and_then(|parent| self.record(parent).ok()).is_some_and(|parent| parent.verification);
        let tasks = request.tasks();
        let mut record = SessionRecord::launched(request, effort, conversation.clone(), verification, now_ms());
        let row = SessionRow { status: Status::Starting.label().to_owned(), ..self.table_row(&record, record.created_ms) };
        self.registrar.register(&row, claim)?;
        let predecessor = self.retire_for(&request.name, claim)?;
        record.addressed_since_ms = predecessor.as_ref().map(Predecessor::addressed_since);
        self.claim_before_start(request, &tasks, verification)?;
        fs::create_dir_all(&session_dir)?;
        if let Some(predecessor) = &predecessor {
            hand_in::inherit_journal(&predecessor.archived, &session_dir)?;
        }
        self.start_claiming(&mut record, &account, conversation.as_deref())?;
        if let Some(task_ref) = &record.task_ref {
            self.ledger.join_task(task_ref, &request.name)?;
        }
        self.deliver_first_turn(&mut record, &account, &request.prompt)?;
        Ok(record)
    }

    fn refuse_second_proxy(&self, request: &LaunchRequest, claim: Claim) -> NorthResult<()> {
        if request.role() != Role::Proxy {
            return Ok(());
        }
        let live = self.sessions()?.into_iter().filter(|record| record.role() == Role::Proxy && self.status(record) != Status::Exited);
        match live.into_iter().find(|record| claim == Claim::Fresh || record.name != request.name) {
            Some(proxy) => Err(NorthError::Usage(format!("{} is the live proxy; a proxy launch with --take-over ends it and moves the role to the new session", proxy.name))),
            None => Ok(()),
        }
    }

    pub fn revive(&self, name: &str, system_prompt: Option<PathBuf>) -> NorthResult<SessionRecord> {
        self.revive_placed(name, system_prompt, None, None)
    }

    /// Revive with a fresh placement override note and effort, which the restarted record keeps.
    pub fn revive_placed(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        self.record(name)?;
        let _start = self.start_lock(name)?;
        self.revive_held(name, system_prompt, placement, effort)
    }

    pub(super) fn revive_held(&self, name: &str, system_prompt: Option<PathBuf>, placement: Option<String>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        let mut record = self.record(name)?;
        record.effort = efforts::revived(&record, self.runtime(&record)?.as_ref(), session_running(&record), effort)?;
        if system_prompt.is_some() {
            record.system_prompt = system_prompt;
        }
        if placement.is_some() {
            record.placement = placement;
        }
        if session_running(&record) {
            return Ok(record);
        }
        self.refuse_beside_interactive(&record)?;
        let conversation = record
            .conversation
            .clone()
            .ok_or_else(|| NorthError::State(format!("{name} never started a conversation")))?;
        let account = self.account(&record.relaunch_request())?;
        self.start(&mut record, &account, Opening::Resume(&conversation))?;
        Ok(record)
    }
}
