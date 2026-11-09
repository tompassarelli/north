use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use time::Date;

use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;
use crate::fleet::install;
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Quota, Status, Transcript};
use crate::fleet::roster::Roster;
use crate::fleet::runtime::{ACTOR_ENV, CAPACITY_NEED_ENV, HostSpec, Instructions, Opening, ROLE_ENV, Runtime, SESSION_ENV, new_uuid, summarize};
use crate::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};
use crate::platform::harness;

use super::route::{self, MODELS, Route, Visibility};
use super::turns::Phase;
use super::{accounts, live, screen};

const PROVIDER: &str = "opencode";
const TOOL_CHARS: usize = 400;
const SETTINGS: &str = "harness/opencode.toml";
const SESSION_CHARS: usize = 26;

type Reading = dyn Fn(&Path) -> Visibility + Send + Sync;
type Clock = dyn Fn() -> Date + Send + Sync;

/// The OpenCode runtime: a worker that runs DeepSeek V4.1 Flash on a lane, with the model chosen here from the lane's repository visibility and never by the agent.
pub struct Opencode {
    visibility: Arc<Reading>,
    today: Arc<Clock>,
    private_lanes: bool,
}

impl Opencode {
    pub fn live() -> Self {
        Self::reading(Arc::new(live::lane_visibility), Arc::new(route::today))
    }

    pub fn reading(visibility: Arc<Reading>, today: Arc<Clock>) -> Self {
        Self { visibility, today, private_lanes: false }
    }

    /// Admits lanes of private repositories and of unreadable visibility, which run on paid Go Flash; the live runtime refuses them until that path lands.
    pub fn with_private_lanes(self) -> Self {
        Self { private_lanes: true, ..self }
    }

    fn route(&self, request: &LaunchRequest, brief: &str) -> NorthResult<Route> {
        let settings = harness::document(SETTINGS);
        let route = Route::of((self.visibility)(&request.cwd));
        if let Some(model) = request.model.as_deref().filter(|model| *model != route.model()) {
            return Err(NorthError::Refused(format!("the launcher fixes this lane's model to {}; {model} was asked for", route.model())));
        }
        if route == Route::Go && !self.private_lanes {
            return Err(NorthError::Refused("opencode workers run public-repository lanes only for now; this lane's repository is private or its visibility could not be read".into()));
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

impl Runtime for Opencode {
    fn provider(&self) -> ProviderName {
        ProviderName::new(PROVIDER)
    }

    fn models(&self) -> &'static [&'static str] {
        MODELS
    }

    fn roster(&self, state_root: &Path, _home: &Path, _policy: &Path) -> Roster {
        accounts::discover(&self.provider(), state_root)
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        Ok(Some(format!("ses_{}", new_uuid()?.replace('-', "").chars().take(SESSION_CHARS).collect::<String>())))
    }

    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec> {
        let conversation = match opening {
            Opening::Fresh(Some(conversation)) | Opening::Resume(conversation) => conversation,
            Opening::Fresh(None) => return Err(NorthError::State("an opencode session needs a conversation id".into())),
        };
        let system = request.system_prompt.as_ref().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        let route = self.route(request, &format!("{}\n{system}\n{instructions}", request.prompt))?;
        let mut argv = vec![north_base::platform::north_binary::north_binary().display().to_string(), "opencode-host".into()];
        argv.extend(["--route".into(), route.label().into(), "--session".into(), conversation.into(), "--scratch".into(), scratch(&request.name).display().to_string()]);
        argv.extend(["--program".into(), install::runtime("opencode").display().to_string()]);
        if let Some(key) = account.token_file.as_ref().filter(|_| self.private_lanes) {
            argv.extend(["--key-file".into(), key.display().to_string()]);
        }
        let mut env: Vec<(String, std::ffi::OsString)> = vec![(SESSION_ENV.into(), request.name.clone().into()), (ROLE_ENV.into(), request.role().label().into()), (ACTOR_ENV.into(), request.name.clone().into())];
        env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
        Ok(HostSpec { key: request.name.clone(), shared: false, argv, env, env_remove: Vec::new(), cwd: request.cwd.clone() })
    }

    fn handshake(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(&self, _name: &str, _request: &LaunchRequest, _opening: Opening, _instructions: Instructions) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn message(&self, _transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>> {
        if !attachments::paths(text).is_empty() {
            return Err(NorthError::Usage("opencode sessions do not take image attachments".into()));
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
        match (frame["type"].as_str(), Phase::of(frame)) {
            (_, Some(Phase::Ready)) if transcript.status == Status::Starting => transcript.status = Status::Idle,
            (_, Some(Phase::Start)) => {
                transcript.status = Status::Working;
                transcript.active_turn = frame["uuid"].as_str().map(str::to_owned);
                push(transcript, EntryKind::User, frame["text"].as_str().unwrap_or_default().to_owned());
            }
            (_, Some(Phase::Fallback)) => push(transcript, EntryKind::Notice, format!("the free model failed ({}); this turn runs again on paid Go Flash", frame["reason"].as_str().unwrap_or("no reason given"))),
            (_, Some(Phase::End)) => observe_end(frame, transcript),
            (Some("text"), _) => push(transcript, EntryKind::Assistant, frame["part"]["text"].as_str().unwrap_or_default().to_owned()),
            (Some("tool_use"), _) => push(transcript, EntryKind::Tool, tool_text(&frame["part"])),
            (Some("error"), _) => push(transcript, EntryKind::Error, frame["error"]["message"].as_str().unwrap_or("error").to_owned()),
            _ => {}
        }
    }

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        let Some(phase) = Phase::of(frame) else { return Vec::new() };
        let Some(conversation) = frame["sessionID"].as_str().filter(|session| !session.is_empty()).map(str::to_owned) else { return Vec::new() };
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

/// How many opencode workers may run at once, apart from the Claude worker cap.
pub fn slots() -> usize {
    harness::document(SETTINGS)["go"]["slots"].as_integer().and_then(|slots| usize::try_from(slots).ok()).unwrap_or(1)
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/north-opencode-{name}"))
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

fn tool_text(part: &Value) -> String {
    let input = &part["state"]["input"];
    let detail = ["command", "filePath", "path", "pattern", "description"].iter().find_map(|key| input[*key].as_str()).map_or_else(|| input.to_string(), str::to_owned);
    format!("{} {}", part["tool"].as_str().unwrap_or("tool"), summarize(&detail, TOOL_CHARS))
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
