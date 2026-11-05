use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use north_landing::land::scans::boxes;

use super::cloud_handback;

pub(super) type Run<'a> = dyn FnMut(&Path, &str, &[&str]) -> Result<String, String> + 'a;

const DROPPED_TRAILERS: [&str; 2] = ["claude-session:", "co-authored-by:"];

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    Queued { lane: PathBuf, output: String },
    Already(String),
    Stopped { reason: String, lane: Option<PathBuf> },
}

impl Outcome {
    pub(super) fn report(&self) -> (u8, String) {
        match self {
            Outcome::Queued { lane, output } => (0, format!("{}\nDone: queued {}", output.trim_end(), lane.display())),
            Outcome::Already(record) => (0, format!("no-op: this handback was already queued: {record}")),
            Outcome::Stopped { reason, lane: Some(lane) } => (1, format!("Stop: {reason}; nothing was queued; lane {}", lane.display())),
            Outcome::Stopped { reason, lane: None } => (1, format!("Stop: {reason}; nothing was queued")),
        }
    }
}

pub(super) fn session_of(events: &[Value], transcript: &Path) -> String {
    events
        .iter()
        .find_map(|event| event.get("session_id").and_then(Value::as_str).filter(|id| !id.is_empty()))
        .map(str::to_owned)
        .unwrap_or_else(|| transcript.file_stem().unwrap_or_default().to_string_lossy().into_owned())
}

pub(super) fn handback_key(session: &str, mbox: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(mbox);
    let last = text
        .lines()
        .filter_map(|line| line.strip_prefix("From ")?.split_whitespace().next())
        .rfind(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))?;
    Some(format!("{session}-{}", &last[..12]))
}

pub(super) fn lane_name(session: &str) -> String {
    let id = session.strip_prefix("session_").unwrap_or(session);
    let safe = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect::<String>();
    format!("cloud-{safe}")
}

pub(super) fn folded_message(log: &str) -> String {
    let kept = log
        .split('\0')
        .map(|message| {
            message
                .lines()
                .filter(|line| !DROPPED_TRAILERS.iter().any(|trailer| line.trim_start().to_ascii_lowercase().starts_with(trailer)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>();
    let (bodies, trailers) = boxes::gather_trailers(&kept.iter().map(String::as_str).collect::<Vec<_>>());
    let mut paragraphs = bodies.into_iter().filter(|message| !message.is_empty()).collect::<Vec<_>>();
    if !trailers.is_empty() {
        paragraphs.push(trailers.join("\n"));
    }
    format!("{}\n", paragraphs.join("\n\n"))
}

pub(super) fn container_of(common_dir: &str) -> Option<PathBuf> {
    Some(Path::new(common_dir.trim()).parent()?.parent()?.to_path_buf())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn land(events: &[Value], transcript: &Path, repo: &Path, state: &Path, run: &mut Run) -> Outcome {
    let stop = |reason: String, lane: Option<&Path>| Outcome::Stopped { reason, lane: lane.map(Path::to_path_buf) };
    let session = session_of(events, transcript);
    let mbox = match cloud_handback::collect(events) {
        Ok(mbox) => mbox,
        Err(error) => return stop(error, None),
    };
    let Some(key) = handback_key(&session, &mbox) else {
        return stop("the handback names no commit".into(), None);
    };
    let record = state.join(format!("{key}.queued"));
    if let Ok(text) = std::fs::read_to_string(&record) {
        return Outcome::Already(text.trim().to_owned());
    }
    if let Err(error) = std::fs::create_dir_all(state) {
        return stop(format!("{}: {error}", state.display()), None);
    }
    let common = match run(repo, "git", &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
        Ok(common) => common,
        Err(error) => return stop(format!("{} is not a git checkout: {error}", repo.display()), None),
    };
    let Some(container) = container_of(&common) else {
        return stop(format!("{} has no project container", repo.display()), None);
    };
    let name = lane_name(&session);
    let lane = container.join("worktrees").join(&name);
    if lane.exists() {
        return stop("an earlier run left this session's lane; finish or abandon it there".into(), Some(&lane));
    }
    let base = match run(repo, "git", &["fetch", "-q", "origin", "main"]).and_then(|_| run(repo, "git", &["rev-parse", "refs/remotes/origin/main"])) {
        Ok(base) => base.trim().to_owned(),
        Err(error) => return stop(format!("cannot fetch origin main: {error}"), None),
    };
    let lane_arg = lane.to_string_lossy().into_owned();
    if let Err(error) = run(repo, "git", &["worktree", "add", "-q", "-b", &name, &lane_arg, &base]) {
        return stop(format!("cannot add the lane off origin/main {base}: {error}"), Some(&lane));
    }
    let mbox_path = state.join(format!("{key}.mbox"));
    if let Err(error) = write(&mbox_path, &mbox) {
        return stop(error, Some(&lane));
    }
    let mbox_arg = mbox_path.to_string_lossy().into_owned();
    if let Err(error) = run(&lane, "git", &["am", "-q", "--3way", &mbox_arg]) {
        return stop(format!("git am --3way could not apply the handback onto origin/main {base}: {error}"), Some(&lane));
    }
    let range = format!("{base}..HEAD");
    let message_path = state.join(format!("{key}.message"));
    let folded = run(&lane, "git", &["log", "--reverse", "--format=%B%x00", &range])
        .and_then(|log| write(&message_path, folded_message(&log).as_bytes()))
        .and_then(|()| run(&lane, "git", &["reset", "-q", "--soft", &base]))
        .and_then(|_| run(&lane, "git", &["commit", "-q", "-F", &message_path.to_string_lossy()]));
    if let Err(error) = folded {
        return stop(format!("cannot fold the handback into one commit: {error}"), Some(&lane));
    }
    if let Err(error) = run(&lane, "north", &["land", "scan", "boxes", &range]) {
        return stop(format!("the box scan refused a Box: trailer: {error}"), Some(&lane));
    }
    if let Err(error) = run(&lane, "north", &["land", "checks", "--base", &base, "--head", "HEAD", "--dry", "1"]) {
        return stop(format!("the lane's land checks failed: {error}"), Some(&lane));
    }
    let output = match run(&lane, "safe-push", &["--to", "main"]) {
        Ok(output) => output,
        Err(error) => return stop(format!("safe-push --to main refused the lane: {error}"), Some(&lane)),
    };
    if let Err(error) = write(&record, format!("{} queued from {} onto origin/main {base}\n", lane.display(), transcript.display()).as_bytes()) {
        return stop(format!("queued, but cannot record it: {error}"), Some(&lane));
    }
    Outcome::Queued { lane, output }
}

fn tail(text: &str) -> String {
    let lines = text.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

pub(super) fn run_program(directory: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let output = if program == "git" {
        crate::platform::git::run_without_local_env(directory, args)
    } else {
        Command::new(program).args(args).current_dir(directory).stdin(Stdio::null()).output()
    }
    .map_err(|error| format!("{program}: {error}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        Err(format!("{program} exited {}: {}", output.status, tail(&text)))
    }
}

pub(super) fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
        .join("north/cloud-handback")
}

pub(super) fn read_events(transcript: &Path) -> Result<Vec<Value>, String> {
    let text = std::fs::read_to_string(transcript).map_err(|error| format!("{}: {error}", transcript.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", transcript.display()))
}

pub(super) fn run(transcript: &Path, repo: &Path, only_handback: bool) -> u8 {
    let events = match read_events(transcript) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("north cloud land: {error}");
            return 1;
        }
    };
    let outcome = if only_handback {
        land_handback(&events, transcript, repo, &state_dir(), &mut run_program)
    } else {
        Some(land(&events, transcript, repo, &state_dir(), &mut run_program))
    };
    let Some(outcome) = outcome else { return 0 };
    let (code, text) = outcome.report();
    println!("{text}");
    code
}

/// The landing `north cloud wait` and `north cloud reap` share: a finished session's transcript
/// lands as `land` does when it holds a handback, and is left alone (`None`) when it does not.
pub(super) fn land_handback(events: &[Value], transcript: &Path, repo: &Path, state: &Path, run: &mut Run) -> Option<Outcome> {
    cloud_handback::has_handback(events).then(|| land(events, transcript, repo, state, run))
}

#[cfg(test)]
#[path = "cloud_land_tests.rs"]
mod tests;
