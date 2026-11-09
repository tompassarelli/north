use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use time::Date;

use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;
use crate::fleet::install;
use crate::fleet::prompt::{PromptInputs, render, seed_orders};
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Quota, Status, Transcript};
use crate::fleet::roster::Roster;
use crate::fleet::runtime::{ACTOR_ENV, CAPACITY_NEED_ENV, DEFAULT_FOREGROUND_CAP_SECS, HostSpec, Instructions, Opening, ROLE_ENV, Runtime, SESSION_ENV, new_uuid};
use crate::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};
use crate::platform::harness;

use super::route::{self, MODELS, Route, Visibility};
use super::harness::{Harness, OPENCODE};
use super::turns::Phase;
use super::lane::{self, Making};
use super::{accounts, live, screen};

pub const PROVIDER: &str = "opencode";
const SETTINGS: &str = "harness/opencode.toml";

type Reading = dyn Fn(&Path) -> Visibility + Send + Sync;
type Clock = dyn Fn() -> Date + Send + Sync;

/// The runtime of a worker that runs DeepSeek V4.1 Flash on a lane through one harness, with the model chosen here from the lane's repository visibility and never by the agent.
pub struct Deepseek {
    harness: &'static dyn Harness,
    visibility: Arc<Reading>,
    today: Arc<Clock>,
    lanes: Arc<Making>,
}

impl Deepseek {
    pub fn opencode() -> Self {
        Self::live(&OPENCODE)
    }

    pub fn live(harness: &'static dyn Harness) -> Self {
        Self::reading(harness, Arc::new(live::lane_visibility), Arc::new(route::today))
    }

    pub fn reading(harness: &'static dyn Harness, visibility: Arc<Reading>, today: Arc<Clock>) -> Self {
        Self { harness, visibility, today, lanes: Arc::new(|_, _| Err("this runtime was built without a lane maker".to_owned())) }
    }

    pub fn with_lanes(self, lanes: Arc<Making>) -> Self {
        Self { lanes, ..self }
    }

    fn route(&self, request: &LaunchRequest, brief: &str) -> NorthResult<Route> {
        let settings = harness::document(SETTINGS);
        let route = Route::of((self.visibility)(&request.cwd));
        if let Some(model) = request.model.as_deref().filter(|model| *model != route.model()) {
            return Err(NorthError::Refused(format!("the launcher fixes this lane's model to {}; {model} was asked for", route.model())));
        }
        let until = route::date(settings["go"]["zero_retention_until"].as_str().unwrap_or_default()).map_err(NorthError::State)?;
        route::admits(route, (self.today)(), until).map_err(NorthError::Refused)?;
        if route == Route::Free {
            let private: Vec<String> = settings["private"]["repos"].as_array().into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
            screen::refuse_for_free(brief, &private).map_err(NorthError::Refused)?;
        }
        Ok(route)
    }
}

impl Runtime for Deepseek {
    fn provider(&self) -> ProviderName {
        ProviderName::new(self.harness.provider())
    }

    fn models(&self) -> &'static [&'static str] {
        MODELS
    }

    fn roster(&self, state_root: &Path, _home: &Path, _policy: &Path) -> Roster {
        accounts::discover(&self.provider(), state_root)
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        Ok(Some(self.harness.conversation(&new_uuid()?)))
    }

    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec> {
        let conversation = match opening {
            Opening::Fresh(Some(conversation)) | Opening::Resume(conversation) => conversation,
            Opening::Fresh(None) => return Err(NorthError::State(format!("a {} session needs a conversation id", self.harness.provider()))),
        };
        let system = request.system_prompt.as_ref().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        let route = self.route(request, &format!("{}\n{system}", request.prompt))?;
        let cwd = lane::prepared(&request.cwd, &request.name, &request.tasks(), &*self.lanes)?;
        let delivered = match route {
            Route::Free => render(&PromptInputs { session: &request.name, role: request.role(), mode: request.prompt_mode, orders: seed_orders(), foreground_cap_secs: request.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS), extra: Some(&system).filter(|text| !text.is_empty()).map(String::as_str) }),
            Route::Go => instructions.to_owned(),
        };
        let mut argv = vec![north_base::platform::north_binary::north_binary().display().to_string(), self.harness.host_verb().into()];
        argv.extend(["--route".into(), route.label().into(), "--session".into(), conversation.into(), "--scratch".into(), scratch(self.harness, &request.name).display().to_string()]);
        argv.extend(["--program".into(), install::runtime(self.harness.provider()).display().to_string()]);
        if matches!(opening, Opening::Fresh(_)) && !delivered.is_empty() {
            let file = scratch(self.harness, &request.name).join("instructions.md");
            std::fs::create_dir_all(scratch(self.harness, &request.name))?;
            std::fs::write(&file, delivered)?;
            argv.extend(["--instructions".into(), file.display().to_string()]);
        }
        if let Some(key) = account.token_file.as_ref() {
            argv.extend(["--key-file".into(), key.display().to_string()]);
        }
        let mut env: Vec<(String, std::ffi::OsString)> = vec![(SESSION_ENV.into(), request.name.clone().into()), (ROLE_ENV.into(), request.role().label().into()), (ACTOR_ENV.into(), request.name.clone().into())];
        env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
        Ok(HostSpec { key: request.name.clone(), shared: false, argv, env, env_remove: Vec::new(), cwd })
    }

    fn handshake(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(&self, _name: &str, _request: &LaunchRequest, _opening: Opening, _instructions: Instructions) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn message(&self, _transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>> {
        if !attachments::paths(text).is_empty() {
            return Err(NorthError::Usage(format!("{} sessions do not take image attachments", self.harness.provider())));
        }
        Ok(vec![json!({"type": "user", "uuid": id, "text": text}).to_string()])
    }

    fn interrupt(&self, _transcript: &Transcript) -> NorthResult<Vec<String>> {
        Ok(vec![json!({"type": "interrupt"}).to_string()])
    }

    fn close(&self, _transcript: &Transcript) -> Vec<String> {
        Vec::new()
    }

    fn observe(&self, frame: &Value, transcript: &mut Transcript) {
        if let Some(session) = frame["sessionID"].as_str().filter(|session| !session.is_empty()) {
            transcript.conversation.get_or_insert_with(|| session.to_owned());
        }
        match Phase::of(frame) {
            Some(Phase::Ready) => {
                if transcript.status == Status::Starting {
                    transcript.status = Status::Idle;
                }
            }
            Some(Phase::Start) => {
                transcript.status = Status::Working;
                transcript.active_turn = frame["uuid"].as_str().map(str::to_owned);
                push(transcript, EntryKind::User, frame["text"].as_str().unwrap_or_default().to_owned());
            }
            Some(Phase::Fallback) => push(transcript, EntryKind::Notice, format!("the free model failed ({}); this turn runs again on paid Go Flash", frame["reason"].as_str().unwrap_or("no reason given"))),
            Some(Phase::End) => observe_end(frame, transcript),
            None => {
                for (kind, text) in self.harness.entries(frame) {
                    push(transcript, kind, text);
                }
            }
        }
    }

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        let Some(conversation) = frame["sessionID"].as_str().filter(|session| !session.is_empty()).map(str::to_owned) else { return Vec::new() };
        let Some(phase) = Phase::of(frame) else { return vec![TurnSignal::Started { conversation }] };
        match phase {
            Phase::Start => {
                let opened = (frame["first"] == true).then(|| TurnSignal::Opened { conversation: conversation.clone(), model: frame["model"].as_str().map(str::to_owned), effort: None });
                opened.into_iter().chain([TurnSignal::Started { conversation: conversation.clone() }, TurnSignal::Prompt { conversation, text: frame["text"].as_str().unwrap_or_default().to_owned() }]).collect()
            }
            Phase::End => vec![TurnSignal::Ended { conversation, exit: exit(frame["exit"].as_str()), totals: TurnTotals { tokens: Some(tokens(&frame["tokens"])), turns: None, wall_ms: frame["wall_ms"].as_u64() } }],
            Phase::Ready | Phase::Fallback => Vec::new(),
        }
    }

    fn frame_conversation<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        frame["sessionID"].as_str().filter(|session| !session.is_empty())
    }

    fn quota(&self, _frame: &Value) -> Option<Quota> {
        None
    }

    fn sent_prompt(&self, frame: &Value) -> Option<String> {
        (frame["type"] == "user").then(|| frame["uuid"].as_str().map(str::to_owned)).flatten()
    }

    fn accepted_prompt(&self, frame: &Value) -> Option<String> {
        (Phase::of(frame) == Some(Phase::Start)).then(|| frame["uuid"].as_str().map(str::to_owned)).flatten()
    }
}

fn scratch(harness: &dyn Harness, name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/north-{}-{name}", harness.provider()))
}

fn exit(label: Option<&str>) -> TurnExit {
    match label {
        Some("interrupted") => TurnExit::Interrupted,
        Some("failed") => TurnExit::Failed,
        _ => TurnExit::Completed,
    }
}

fn tokens(counts: &Value) -> Tokens {
    let count = |key: &str| counts[key].as_u64().unwrap_or(0);
    Tokens { input: count("input"), output: count("output"), cache_read: count("cache_read"), cache_write: count("cache_write") }
}

fn observe_end(frame: &Value, transcript: &mut Transcript) {
    transcript.status = Status::Idle;
    transcript.active_turn = None;
    let exit = frame["exit"].as_str().unwrap_or("completed");
    if let Some(error) = frame["error"].as_str().filter(|_| exit == "failed") {
        push(transcript, EntryKind::Error, error.to_owned());
    }
    push(transcript, EntryKind::TurnEnd, exit.to_owned());
}

fn push(transcript: &mut Transcript, kind: EntryKind, text: String) {
    if !text.is_empty() {
        transcript.entries.push(Entry { kind, text });
    }
}
