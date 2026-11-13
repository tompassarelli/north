use crate::error::{NorthError, NorthResult};
use crate::fleet::handover::family_of;
use crate::fleet::model::{Delivery, Effort, EntryKind, LaunchRequest, ProviderName, SessionRecord, Status, Transcript};
use crate::fleet::records::write_record;
use crate::fleet::remote::HooksCheck;
use crate::fleet::runtime::Opening;
use crate::fleet::sender::Sender;

use super::stopping::Halt;
use super::{Fleet, efforts, session_running};

/// The words a turn's report begins with, which a relaunch across model families carries into the fresh conversation.
const REPORT_WORDS: [&str; 4] = ["Done:", "Not done:", "Split:", "Blocked:"];

/// How a relaunch restarts the session: its own conversation under the new model, or a fresh one that carries the old one's last report, since a model of another family cannot read the thinking the conversation holds.
pub enum Restart {
    Resume,
    Fresh { conversation: Option<String>, follows: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choices {
    pub provider: ProviderName,
    pub models: Vec<String>,
    pub takes_effort: bool,
}

impl Fleet {
    /// The models and efforts each provider accepts for a session, as `relaunch` checks them.
    pub fn choices(&self) -> Vec<Choices> {
        self.providers()
            .into_iter()
            .filter_map(|provider| {
                let runtime = self.runtimes.get(&provider).ok()?;
                Some(Choices { provider, models: runtime.models().iter().map(|model| (*model).to_owned()).collect(), takes_effort: runtime.takes_effort() })
            })
            .collect()
    }
}

impl Fleet {
    /// Restarts a session on the same conversation under another model or effort, or on a fresh conversation when the model belongs to another family than the one it ran, since the new model cannot read the thinking the old conversation holds. Both are checked as a launch checks them before the session is stopped, so a refused change leaves it running.
    pub fn relaunch(&self, name: &str, model: Option<&str>, effort: Option<Effort>) -> NorthResult<SessionRecord> {
        self.relaunch_with(name, model, effort, |name, restart| match restart {
            Restart::Resume => self.revive_held(name, None, None, None),
            Restart::Fresh { follows, .. } => {
                let revived = self.start_fresh(name)?;
                if revived.conversation.is_some() {
                    self.send_signed(&Sender::fleet(), name, &follows, Delivery::Queued)?;
                }
                Ok(revived)
            }
        })
    }

    pub(crate) fn relaunch_with(&self, name: &str, model: Option<&str>, effort: Option<Effort>, restart: impl FnOnce(&str, Restart) -> NorthResult<SessionRecord>) -> NorthResult<SessionRecord> {
        let record = self.record(name)?;
        if model.is_none() && effort.is_none() {
            return Err(NorthError::Usage("relaunch takes a model, an effort or both".into()));
        }
        let runtime = self.runtime(&record)?;
        if let Some(model) = model.filter(|model| !runtime.models().contains(model)) {
            return Err(NorthError::Usage(format!("{} has no model {model}; choose one of {}", record.provider.label(), runtime.models().join(", "))));
        }
        let mut changed = SessionRecord { model: model.map(str::to_owned).or_else(|| record.model.clone()), ..record.clone() };
        changed.effort = efforts::revived(&changed, runtime.as_ref(), false, effort)?;
        let restarting = if family_of(record.model.as_deref()) == family_of(changed.model.as_deref()) {
            Restart::Resume
        } else {
            Restart::Fresh { conversation: runtime.assign_conversation()?, follows: across_families(&record, &changed, &self.recent(runtime.as_ref(), &record)?) }
        };
        let fresh = matches!(restarting, Restart::Fresh { .. });
        self.refuse_beside_interactive(&record)?;
        let cut_short = self.halt_cut_short(name, self.ordered_by(self.caller().as_deref(), Halt::Relaunched))?;
        let restarted = {
            let _start = self.start_lock(name)?;
            let mut stopped = self.record(name)?;
            (stopped.model, stopped.effort) = (changed.model.clone(), changed.effort);
            if let Restart::Fresh { conversation, .. } = &restarting {
                stopped.conversation = conversation.clone();
            }
            write_record(&self.session_dir(name)?, &stopped)?;
            restart(name, restarting)?
        };
        if cut_short && !fresh {
            self.carry_on(name, &relaunched_on(&changed))?;
        }
        Ok(restarted)
    }

    /// Starts `name` on the fresh conversation its record now holds, which records the system prompt as it stands instead of resuming the one the old model wrote.
    fn start_fresh(&self, name: &str) -> NorthResult<SessionRecord> {
        let mut record = self.record(name)?;
        let account = self.account(&LaunchRequest::relaunching(&record))?;
        let conversation = record.conversation.clone();
        let hooks = HooksCheck::start(self.runtime(&record)?, &account, record.machine.as_ref());
        self.start(&mut record, &account, Opening::Fresh(conversation.as_deref()), hooks)?;
        Ok(record)
    }

    /// Whether `name`'s host is gone and its conversation stopped in the middle of a turn, which a restart without a message of its own leaves waiting.
    pub fn left_mid_turn(&self, name: &str) -> NorthResult<bool> {
        let record = self.record(name)?;
        Ok(!session_running(&record) && self.recent(self.runtime(&record)?.as_ref(), &record)?.status == Status::Working)
    }

    /// Hands a restarted session the turn that carries on the one `why` cut short, since its CLI resumes the conversation without running anything until a message arrives.
    pub fn carry_on(&self, name: &str, why: &str) -> NorthResult<()> {
        self.send_signed(&Sender::fleet(), name, &format!("[north: {why}] Carry on with that turn from where it stopped."), Delivery::Queued)
    }
}

fn relaunched_on(record: &SessionRecord) -> String {
    let model = record.model.as_deref().unwrap_or("its default model");
    let effort = record.effort.map_or(String::new(), |effort| format!(" at {} effort", effort.label()));
    format!("this session was relaunched on {model}{effort} in the middle of a turn")
}

/// The brief a relaunch across model families hands the fresh conversation: what it follows and the report the old model ended on.
fn across_families(record: &SessionRecord, changed: &SessionRecord, transcript: &Transcript) -> String {
    let follows = record.task_ref.clone().unwrap_or_else(|| record.name.clone());
    let from = record.model.as_deref().unwrap_or("its default model");
    let to = changed.model.as_deref().unwrap_or("its default model");
    let report = last_report(transcript).unwrap_or_else(|| "It left no report.".to_owned());
    format!("Follows: {follows} (relaunched from {from} to {to}, whose family cannot read the conversation the first model wrote)\n\n{report}")
}

fn last_report(transcript: &Transcript) -> Option<String> {
    transcript.entries.iter().rev().find(|entry| entry.kind == EntryKind::Assistant && REPORT_WORDS.iter().any(|word| entry.text.trim_start().starts_with(word))).map(|entry| entry.text.clone())
}

/// Why a revive that brings no message of its own hands the session a turn.
pub const REVIVED_MID_TURN: &str = "this session was revived after its host exited in the middle of a turn";
