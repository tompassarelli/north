use super::bash::{check_key, check_name, gh_issue_action, is_check, is_measure, issue_body, segments};
use super::case::parse_case;
use super::state::{self, count, head_commit, tree_state, truthy, unlanded, with_shared, with_state};
use super::{Ctx, PEER_LINE, pattern};
use crate::hook::{HookInput, Verdict};
use crate::platform::pyjson::{self, Value as Py};
use serde_json::{Map, Value, json};

const READ_ONLY_WORDS: [&str; 24] = [
    "rg", "grep", "egrep", "cat", "head", "tail", "less", "ls", "find", "fd", "tree", "wc", "jq", "file", "stat", "du", "diff", "cut", "sort", "uniq", "convo", "pwd", "which", "realpath",
];
const READ_ONLY_GIT: [&str; 10] = ["log", "show", "diff", "status", "blame", "grep", "ls-files", "rev-parse", "branch", "remote"];
const STREAK_LIMIT: i64 = 20;
const LAND_NUDGE: i64 = 3;

pattern!(EXIT_CODE, r"(?:Exit code:|Process exited with code)\s*(-?\d+)");
pattern!(LAND_CHECK, r"\b(test|tests|check|typecheck|farm|sweep|parity|verify|validate)\b");
pattern!(LANDING, r"\b(git commit|safe-push|git push)\b");
pattern!(ISSUE_REF, r"^(?:#?\d+|https?://\S+/issues/\d+)$");
pattern!(ISSUE_NUMBER, r"^(?:#?\d+)$");
pattern!(TICKED, r"(?m)^\s*[-*] \[[xX]\] ");
pattern!(OPEN_BOX, r"(?m)^\s*[-*] \[ \] ");
pattern!(CLOSED_STATE, r#"(?i)\bstate:\s*closed\b|"state":\s*"CLOSED""#);

fn read_only(command: &str) -> bool {
    let parts = segments(command);
    !parts.is_empty()
        && parts.iter().all(|words| {
            let head = super::bash::basename(words[0]);
            head == "cd"
                || (head == "sed" && words.contains(&"-n") && !words.contains(&"-i"))
                || (head == "git" && words.get(1).is_some_and(|w| READ_ONLY_GIT.contains(w)))
                || (head == "gh" && words.get(2).is_some_and(|w| *w == "view" || *w == "list"))
                || (head == "gh" && words.get(1) == Some(&"api") && !words.contains(&"-X") && !words.contains(&"--method"))
                || READ_ONLY_WORDS.contains(&head)
        })
}

fn response_text(input: &HookInput) -> String {
    match input.data.get("tool_response") {
        Some(Py::String(text)) => text.lossy().into_owned(),
        Some(value) => pyjson::dumps(value),
        None => "null".into(),
    }
}

fn add(state: &mut Map<String, Value>, key: &str, by: i64) -> i64 {
    let next = count(state.get(key)) + by;
    state.insert(key.into(), json!(next));
    next
}

struct Run<'a> {
    command: &'a str,
    cwd: &'a Value,
    code: Option<i64>,
}

fn record_check(ctx: &Ctx, state: &mut Map<String, Value>, run: &Run, notes: &mut Vec<String>) {
    let key = check_key(run.cwd, run.command);
    let cwd = run.cwd.as_str().unwrap_or("None");
    if run.code != Some(0) {
        if let Some(passed) = state.get_mut("passed").and_then(Value::as_object_mut) {
            passed.remove(&key);
        }
        return;
    }
    let tree = tree_state(cwd);
    if let (Some(tree), Some(passed)) = (&tree, state.get_mut("passed").and_then(Value::as_object_mut)) {
        passed.insert(key, json!({"tree": tree, "at": ctx.now}));
    }
    if let Some(commit) = head_commit(cwd) {
        let fingerprint = format!("{commit}\0{}", check_name(run.command));
        with_shared(ctx, |shared| {
            shared.entry(fingerprint).or_insert_with(|| json!({"at": ctx.now}));
        });
    }
    if LAND_CHECK.is_match(run.command) && unlanded(cwd) {
        state.insert("last_pass".into(), json!({"cwd": run.cwd, "tree": tree, "at": ctx.now}));
        state.insert("land_wait".into(), json!(0));
        notes.push("That check passed. Land it now: commit, `safe-push --to main`, then tick the box. No more checks first.".into());
    }
}

fn track_landing(ctx: &Ctx, state: &mut Map<String, Value>, run: &Run, notes: &mut Vec<String>) {
    if is_check(run.command) && run.code.is_some() {
        record_check(ctx, state, run, notes);
    } else if LANDING.is_match(run.command) && run.code == Some(0) {
        state.remove("land_wait");
    } else if state.get("land_wait").is_some_and(|wait| !wait.is_null()) && add(state, "land_wait", 1) >= LAND_NUDGE {
        state.remove("land_wait");
        notes.push(format!("{LAND_NUDGE} commands since a passing check and still nothing landed. Land now, or say in one line which box is still failing."));
    }
}

fn ticked_issue(input: &HookInput, run: &Run, action: &str, words: &[String]) -> Option<String> {
    let text = if action == "edit" { issue_body(run.command, run.cwd.as_str()) } else { response_text_or_empty(input) };
    if !TICKED.is_match(&text) || OPEN_BOX.is_match(&text) || CLOSED_STATE.is_match(&text) {
        return None;
    }
    let number = words.iter().skip(3).find(|w| ISSUE_NUMBER.is_match(w)).map_or("it", String::as_str);
    Some(format!(
        "Every box on issue {number} is ticked. Close it now: `gh issue close {number} --comment \"<run and commit>\"`. Don't run anything else first. {PEER_LINE}"
    ))
}

fn response_text_or_empty(input: &HookInput) -> String {
    match input.data.get("tool_response") {
        Some(value) if !matches!(value, Py::Null | Py::Bool(false)) && value.as_str() != Some("") => response_text(input),
        _ => String::new(),
    }
}

fn eta_note(ctx: &Ctx, state: &mut Map<String, Value>) -> Option<String> {
    let eta = state.get_mut("eta").filter(|eta| truthy(Some(eta)))?.as_object_mut()?;
    if truthy(eta.get("warned")) {
        return None;
    }
    let spent = ctx.now - eta.get("set_at").and_then(Value::as_f64).unwrap_or(0.0);
    let minutes = eta.get("minutes").and_then(Value::as_f64).unwrap_or(0.0);
    if spent <= 2.0 * minutes * 60.0 {
        return None;
    }
    eta.insert("warned".into(), json!(true));
    Some(format!(
        "You're at {} on a {} min ETA, more than twice over. Stop expanding. Land what passes now, close what's done, and report the rest with a new ETA and the reason.",
        super::minutes(spent),
        count(eta.get("minutes"))
    ))
}

pub(super) fn post_bash(ctx: &Ctx, input: &HookInput, event: &Value) -> Verdict {
    let full = super::command(event);
    let run = Run {
        command: parse_case(&full).bare,
        cwd: &event["cwd"],
        code: EXIT_CODE.captures(&response_text(input)).and_then(|m| m[1].parse().ok()),
    };
    let notes = with_state(ctx, &state::agent_key(event), |state| {
        let mut notes = Vec::new();
        if read_only(run.command) {
            if add(state, "streak", 1) == STREAK_LIMIT {
                notes.push(format!("You've run {STREAK_LIMIT} read-only commands since your last change or run. Stop reading. Make the change you think is right and let the build or test tell you if it's wrong."));
            }
        } else {
            state.insert("streak".into(), json!(0));
        }
        let issue = gh_issue_action(run.command);
        let action = issue.as_ref().map_or("", |(action, _)| action.as_str());
        if run.code == Some(0) && action == "create" {
            add(state, "opened", 1);
        }
        if run.code == Some(0) && action == "close" {
            let numbers = issue.as_ref().map_or(0, |(_, words)| words.iter().skip(3).filter(|w| ISSUE_REF.is_match(w)).count());
            add(state, "closed", numbers.max(1) as i64);
            state.insert("last_close".into(), json!(ctx.now));
        }
        track_landing(ctx, state, &run, &mut notes);
        if is_measure(run.command) && run.code.is_some() {
            add(state, "measures", 1);
        }
        if let Some((action, words)) = issue.as_ref().filter(|(a, _)| ["edit", "view", "comment"].contains(&a.as_str()) && run.code == Some(0)) {
            notes.extend(ticked_issue(input, &run, action, words));
        }
        notes.extend(eta_note(ctx, state));
        notes
    });
    if notes.is_empty() { Verdict::Allow } else { Verdict::Advise(notes.join(" ")) }
}

pub(super) fn post_spawn(ctx: &Ctx, event: &Value) -> Verdict {
    with_state(ctx, &state::agent_key(event), |state| add(state, "spawned", 1));
    Verdict::Allow
}

pub(super) fn post_patch(ctx: &Ctx, event: &Value) -> Verdict {
    with_state(ctx, &state::agent_key(event), |state| {
        state.insert("streak".into(), json!(0));
        state.insert("measures".into(), json!(0));
        state.remove("fix_baseline");
    });
    Verdict::Allow
}
