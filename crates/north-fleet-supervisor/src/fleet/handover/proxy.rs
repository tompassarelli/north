//! The proxy's move to another account, however it was started: the tick asks it once for a hand-over and lets it end its turn, then
//! launches its successor on the new account with that hand-over as its first prompt and stops it; when no hand-over came within the
//! deadline, the same conversation resumes on the new account instead. A turn in progress is never cut short.
use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::attribution::{self, Cause, Harness};
use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::fleet::model::{Delivery, Effort, LaunchRequest, SessionRecord, Status};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;
use crate::usage::decide::world::ModelFamily;

pub const DEADLINE_MS: u64 = 5 * 60_000;
const DESK: &str = ".handovers";

#[derive(Debug, Eq, PartialEq)]
pub enum Step {
    Ask,
    Wait,
    Launch(String),
    Resume,
}

pub fn step(asked_ms: Option<u64>, written: Option<String>, turn_ended: bool, now_ms: u64) -> Step {
    let Some(asked_ms) = asked_ms else { return Step::Ask };
    if !turn_ended {
        return Step::Wait;
    }
    match written {
        Some(handover) => Step::Launch(handover),
        None if now_ms.saturating_sub(asked_ms) >= DEADLINE_MS => Step::Resume,
        None => Step::Wait,
    }
}

#[derive(Deserialize, Serialize)]
struct Asked {
    conversation: Option<String>,
    asked_ms: u64,
}

/// Where one proxy writes its hand-over and where the tick notes that it asked for it, outside the session directory a take-over archives.
pub struct Desk {
    dir: PathBuf,
    name: String,
}

impl Desk {
    pub fn of(fleet_root: &Path, name: &str) -> Self {
        Self { dir: fleet_root.join(DESK), name: name.to_owned() }
    }

    pub fn file(&self) -> PathBuf {
        self.dir.join(format!("{}.md", self.name))
    }

    fn asked_file(&self) -> PathBuf {
        self.dir.join(format!("{}.asked.json", self.name))
    }

    fn asked(&self, conversation: Option<&str>) -> Option<u64> {
        let asked: Asked = serde_json::from_slice(&fs::read(self.asked_file()).ok()?).ok()?;
        (asked.conversation.as_deref() == conversation).then_some(asked.asked_ms)
    }

    fn ask(&self, conversation: Option<&str>, now_ms: u64) -> NorthResult<()> {
        fs::create_dir_all(&self.dir)?;
        let _ = fs::remove_file(self.file());
        fs::write(self.asked_file(), serde_json::to_vec(&Asked { conversation: conversation.map(str::to_owned), asked_ms: now_ms })?)?;
        Ok(())
    }

    fn written(&self) -> Option<String> {
        fs::read_to_string(self.file()).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
    }

    fn keep(&self, asked_ms: u64) {
        let _ = fs::rename(self.file(), self.dir.join(format!("{}-{asked_ms}.md", self.name)));
        self.clear();
    }

    pub fn clear(&self) {
        let _ = fs::remove_file(self.asked_file());
    }
}

/// Takes the proxy `record` one step toward account `to` and says what it did; None while it waits for its hand-over or its turn.
pub fn advance(fleet: &Fleet, record: &SessionRecord, to: &str, family: Option<ModelFamily>, take_over: &dyn Fn(&LaunchRequest) -> NorthResult<SessionRecord>, now_ms: u64) -> Option<String> {
    let desk = Desk::of(&fleet.root, &record.name);
    let (name, from) = (&record.name, &record.account);
    let conversation = record.conversation.as_deref();
    let asked_ms = desk.asked(conversation);
    let request = |prompt: String, resume: Option<String>| {
        let mut request = LaunchRequest { account: to.to_owned(), prompt, placement: Some(format!("handed over from account {from}")), resume, ..record.relaunch_request() };
        if let Some(family) = family {
            request.model = Some(model_name(family).to_owned());
            request.effort = Some(Effort::High);
        }
        request
    };
    match step(asked_ms, desk.written(), fleet.status(record) != Status::Working, now_ms) {
        Step::Wait => None,
        Step::Ask => Some(match desk.ask(conversation, now_ms).and_then(|()| fleet.send_signed(&Sender::fleet(), name, &ask(record, to, &desk.file()), Delivery::Now)) {
            Ok(()) => format!("{name}: asked for its hand-over at {} before it moves from account {from} to {to}", desk.file().display()),
            Err(error) => {
                desk.clear();
                format!("{name}: the hand-over request did not reach it, so the next tick asks again: {}", error.user_message())
            }
        }),
        Step::Launch(handover) => Some(match take_over(&request(successor(record, to, &handover), None)) {
            Ok(_) => {
                moved(fleet, record, to);
                desk.keep(asked_ms.unwrap_or(now_ms));
                let closed = if record.adopted() { fleet.close_window(record).map_or_else(|error| format!("; its old window stays open: {}", error.user_message()), |_| "; its old window is closed".to_owned()) } else { String::new() };
                format!("{name}: handed over from account {from} to {to} as a new conversation that starts from its hand-over{closed}")
            }
            Err(error) => format!("{name}: not handed over from account {from} to {to}: {}", error.user_message()),
        }),
        Step::Resume => Some(match free(fleet, record).and_then(|()| take_over(&request(resumed(record, to), record.conversation.clone()))) {
            Ok(_) => {
                moved(fleet, record, to);
                desk.clear();
                format!("{name}: wrote no hand-over within {} min, so its conversation resumes on account {to} instead of {from}", DEADLINE_MS / 60_000)
            }
            Err(error) => format!("{name}: not handed over from account {from} to {to}: {}", error.user_message()),
        }),
    }
}

/// The model name a family switch writes on the successor's record.
fn model_name(family: ModelFamily) -> &'static str {
    match family {
        ModelFamily::Fable => "fable",
        ModelFamily::Opus => "opus",
        ModelFamily::Sonnet => "sonnet",
        ModelFamily::Haiku => "haiku",
    }
}

fn moved(fleet: &Fleet, record: &SessionRecord, to: &str) {
    attribution::record(&record.name, "fleet", Cause::Harness(Harness::Capacity), &format!("handed over from account {} to {to}", record.account));
    let _ = fleet.waits().clear_usage(&record.name);
}

fn free(fleet: &Fleet, record: &SessionRecord) -> NorthResult<()> {
    if record.adopted() {
        fleet.close_window(record)?;
    }
    Ok(())
}

fn ask(record: &SessionRecord, to: &str, file: &Path) -> String {
    format!(
        "[north: account {from} is at or past its floor, so `north fleet tick` moves {name} to account {to}, the source above the floor with the most headroom] \
         Write your hand-over for the next proxy to {file} now: what is in flight, who owns each open item, what waits on whom, and the next steps. Then end your turn. \
         Once your turn has ended, the tick starts the new proxy on account {to} with that file as its first prompt and stops this session. \
         With no file after {minutes} min, this conversation resumes on account {to} instead.",
        from = record.account,
        name = record.name,
        file = file.display(),
        minutes = DEADLINE_MS / 60_000,
    )
}

fn successor(record: &SessionRecord, to: &str, handover: &str) -> String {
    format!(
        "[north: account {from} is at or past its floor, so `north fleet tick` moved {name} to account {to}, the source above the floor with the most headroom, as a new conversation. The previous proxy wrote this hand-over:]\n\n{handover}",
        from = record.account,
        name = record.name,
    )
}

pub(super) fn resumed(record: &SessionRecord, to: &str) -> String {
    format!(
        "[north: account {from} is at or past its floor, so `north fleet tick` moved {name} onto account {to}, the source above the floor with the most headroom] This is the same conversation; carry on where it stopped.",
        from = record.account,
        name = record.name,
    )
}
