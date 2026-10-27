mod spawn;
mod transcript;

use crate::hook::{HookInput, Verdict};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const STEP: i64 = 50_000;
const LEASH_STEP_MIN: i64 = 10;

pub type Confirm = fn(&serde_json::Value) -> Result<(), String>;

static CONFIRM: OnceLock<Confirm> = OnceLock::new();

pub fn install_confirm(confirm: Confirm) {
    let _ = CONFIRM.set(confirm);
}

pub(super) struct Env<'a> {
    pub home: PathBuf,
    pub now: f64,
    pub off: &'a dyn Fn() -> bool,
    pub confirm: Option<Confirm>,
}

pub fn evaluate(input: &HookInput) -> Verdict {
    let off = || crate::hook::authoring_guards_off("north-hook");
    let home = std::env::home_dir().unwrap_or_default();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let env = Env { home, now, off: &off, confirm: CONFIRM.get().copied() };
    decide(input, &env).map_or(Verdict::Allow, Verdict::Advise)
}

pub(super) fn decide(input: &HookInput, env: &Env) -> Option<String> {
    if input.data["hook_event_name"] == "PostToolUse" && input.data["tool_name"] == "Agent" {
        return spawn::record(input, env);
    }
    handoff(input, env)
}

pub(super) fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn handoff(input: &HookInput, env: &Env) -> Option<String> {
    let data = &input.data;
    data["agent_type"].as_str().filter(|kind| kind.starts_with("worker"))?;
    let agent_id = data["agent_id"].as_str().filter(|id| valid_id(id))?;
    let session = data["transcript_path"].as_str()?.strip_suffix(".jsonl")?;
    let transcript = Path::new(session).join("subagents").join(format!("agent-{agent_id}.jsonl"));
    let threshold = crate::pace::limits::handoff_ctx() as i64;
    let leash = crate::pace::limits::overtime_min() as i64;
    let tokens = transcript::context_tokens(&transcript);
    let minutes = transcript::started_at(&transcript).map_or(0, |start| ((env.now - start) / 60.0).floor() as i64);
    let (remind, overdue) = (tokens >= threshold, minutes >= leash);
    if !(remind || overdue) || (env.off)() {
        return None;
    }
    let handoffs = env.home_dir().join(".local/state/agents/handoffs");
    let note = handoffs.join(format!("{agent_id}.md")).display().to_string();
    let mut messages = Vec::new();
    if remind && due(&handoffs.join(format!(".{agent_id}.reminded")), tokens, STEP) {
        messages.push(format!(
            "Your context is {}k tokens, past the {}k handoff point. Land anything that passes now; if work remains, finish the step you are on, then write a handoff note to {note} with: the brief's goal and Done when list with each box's status; the worktree, branch and commits; running background jobs with their output paths; the next step; evidence paths. Then end your turn with exactly `HANDOFF {note}` and nothing else.",
            tokens / 1000,
            threshold / 1000,
        ));
    }
    if overdue && due(&handoffs.join(format!(".{agent_id}.leash")), minutes, LEASH_STEP_MIN) {
        messages.push(format!(
            "You have run {leash} minutes. Land anything that passes now, then either report (Done:/Not done:/Blocked:) or write a handoff to ~/.local/state/agents/handoffs/<lane>.md, commit your WIP and report Not done: with its path."
        ));
    }
    (!messages.is_empty()).then(|| messages.join(" "))
}

impl Env<'_> {
    fn home_dir(&self) -> PathBuf {
        let text = self.home.to_string_lossy();
        let trimmed = text.trim_end_matches('/');
        PathBuf::from(if trimmed.is_empty() { "/" } else { trimmed })
    }
}

fn due(marker: &Path, value: i64, step: i64) -> bool {
    let last = std::fs::read_to_string(marker)
        .ok()
        .map_or(0, |text| text.trim().parse::<i64>().unwrap_or(0));
    if last != 0 && value < last + step {
        return false;
    }
    let Some(directory) = marker.parent() else {
        return false;
    };
    std::fs::create_dir_all(directory).is_ok() && std::fs::write(marker, format!("{value}\n")).is_ok()
}
