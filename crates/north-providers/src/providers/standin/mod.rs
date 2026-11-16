//! The stand-in worker (north#1632): a session that calls no model. Each turn it reads the brief's `Item:` and `Box:`
//! lines, makes a lane in the item's repository that writes one file under `lanes/standin/`, and hands it in, so
//! dispatch, the hand-in and the lander run at a chosen rate with no token spent.
use std::path::Path;

use serde_json::{Value, json};

use north_foundation::error::{NorthError, NorthResult};
use north_fleet_supervisor::fleet::model::{Account, Entry, EntryKind, LaunchRequest, Machine, ProviderName, Quota, Status, Transcript};
use north_fleet_supervisor::fleet::roster::{Declared, Roster};
use north_fleet_supervisor::fleet::runtime::{ACTOR_ENV, HostSpec, Instructions, Opening, ROLE_ENV, Runtime, SESSION_ENV, new_uuid};
use north_fleet_supervisor::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};

mod host;

pub use host::run;

pub const PROVIDER: &str = "standin";
pub const ACCOUNT: &str = "local";
pub const HOST_VERB: &str = "standin-host";
const MODEL: &str = "standin";
const STANDIN: &str = "standin";

pub struct Standin;

fn account(provider: ProviderName, state_root: &Path) -> Account {
    Account { provider, label: ACCOUNT.into(), home: state_root.join("accounts").join(PROVIDER), token_file: None, api: None }
}

impl Runtime for Standin {
    fn provider(&self) -> ProviderName {
        ProviderName::new(PROVIDER)
    }

    fn models(&self) -> &'static [&'static str] {
        &[MODEL]
    }

    fn roster(&self, state_root: &Path, _home: &Path, _policy: &Path) -> Roster {
        Roster::distinct([(account(self.provider(), state_root), None)], &Declared::default())
    }

    fn account_on(&self, home: &Path, _policy: &Path, label: &str, machine: &Machine) -> NorthResult<Account> {
        if label != ACCOUNT {
            return Err(NorthError::Usage(format!("no {PROVIDER} account named {label} exists for {}; the one account is {ACCOUNT}", machine.name)));
        }
        Ok(account(self.provider(), &home.join(".local/state/north")))
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        new_uuid().map(Some)
    }

    fn host(&self, _account: &Account, request: &LaunchRequest, opening: Opening, _instructions: &str) -> NorthResult<HostSpec> {
        let conversation = match opening {
            Opening::Fresh(Some(conversation)) | Opening::Resume(conversation) => conversation,
            Opening::Fresh(None) => return Err(NorthError::State("a stand-in session needs a conversation id".into())),
        };
        admitted(request)?;
        let argv = vec![north_base::platform::north_binary::north_binary().display().to_string(), HOST_VERB.into(), "--session".into(), conversation.into()];
        let env = vec![(SESSION_ENV.into(), request.name.clone().into()), (ROLE_ENV.into(), request.role().label().into()), (ACTOR_ENV.into(), request.name.clone().into())];
        Ok(HostSpec { key: request.name.clone(), shared: false, argv, env, env_remove: Vec::new(), cwd: request.cwd.clone() })
    }

    fn handshake(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(&self, _name: &str, _request: &LaunchRequest, _opening: Opening, _instructions: Instructions) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn message(&self, _transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>> {
        Ok(vec![json!({"type": "user", "uuid": id, "text": text}).to_string()])
    }

    fn interrupt(&self, _transcript: &Transcript) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn close(&self, _transcript: &Transcript) -> Vec<String> {
        Vec::new()
    }

    fn observe(&self, frame: &Value, transcript: &mut Transcript) {
        if let Some(session) = host::session(frame) {
            transcript.conversation.get_or_insert_with(|| session.to_owned());
        }
        match host::phase(frame) {
            Some("ready") if transcript.status == Status::Starting => transcript.status = Status::Idle,
            Some("start") => {
                transcript.status = Status::Working;
                transcript.active_turn = frame["uuid"].as_str().map(str::to_owned);
                push(transcript, EntryKind::User, frame["text"].as_str().unwrap_or_default());
            }
            Some("end") => {
                transcript.status = Status::Idle;
                transcript.active_turn = None;
                push(transcript, EntryKind::TurnEnd, frame["exit"].as_str().unwrap_or("completed"));
            }
            Some(_) => {}
            None if frame["type"] == "text" => push(transcript, EntryKind::Assistant, frame["text"].as_str().unwrap_or_default()),
            None => {}
        }
    }

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        let Some(conversation) = host::session(frame).map(str::to_owned) else { return Vec::new() };
        match host::phase(frame) {
            Some("start") => {
                let opened = (frame["first"] == true).then(|| TurnSignal::Opened { conversation: conversation.clone(), model: Some(MODEL.into()), effort: None });
                opened.into_iter().chain([TurnSignal::Started { conversation: conversation.clone() }, TurnSignal::Prompt { conversation, text: frame["text"].as_str().unwrap_or_default().to_owned() }]).collect()
            }
            Some("end") => {
                let exit = if frame["exit"] == "failed" { TurnExit::Failed } else { TurnExit::Completed };
                vec![TurnSignal::Ended { conversation, exit, totals: TurnTotals { tokens: Some(Tokens::default()), wall_ms: frame["wall_ms"].as_u64(), ..TurnTotals::default() } }]
            }
            _ => Vec::new(),
        }
    }

    fn frame_conversation<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        host::session(frame)
    }

    fn quota(&self, _frame: &Value) -> Option<Quota> {
        None
    }

    fn sent_prompt(&self, frame: &Value) -> Option<String> {
        (frame["type"] == "user").then(|| frame["uuid"].as_str().map(str::to_owned)).flatten()
    }

    fn accepted_prompt(&self, frame: &Value) -> Option<String> {
        (host::phase(frame) == Some("start")).then(|| frame["uuid"].as_str().map(str::to_owned)).flatten()
    }
}

fn listed(table: &toml_edit::DocumentMut, key: &str) -> Vec<String> {
    table.get(STANDIN).and_then(|standin| standin.get(key)).and_then(toml_edit::Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect()
}

fn admitted(request: &LaunchRequest) -> NorthResult<()> {
    let policy = north_base::platform::orchestration::snapshot().map_err(NorthError::Refused)?;
    let repo = request.cwd.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let host = request.machine.as_ref().map_or_else(north_base::platform::host::name, |machine| machine.name.clone());
    if !listed(&policy, "repos").iter().any(|listed| listed.eq_ignore_ascii_case(&repo)) {
        return Err(NorthError::Refused(format!("a stand-in runs only on a repository the orchestration file's [{STANDIN}] repos names, not {repo}")));
    }
    if !listed(&policy, "hosts").contains(&host) {
        return Err(NorthError::Refused(format!("a stand-in runs only on a host the orchestration file's [{STANDIN}] hosts names, not {host}")));
    }
    Ok(())
}

fn push(transcript: &mut Transcript, kind: EntryKind, text: &str) {
    if !text.is_empty() {
        transcript.entries.push(Entry { kind, text: text.to_owned() });
    }
}
