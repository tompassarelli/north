use crate::guard::host::{self, ReadStreak};
use crate::fleet::runtime::{PROXY, ROLE_ENV, SESSION_ENV};
use crate::guard::paths::{basename, expanduser, join, normpath};
use crate::hook_input::{HookInput, Verdict};
use crate::shell::{blank_quotes, strip_heredocs};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

const UNIT: &str = "proxy-role-guard";
const STREAK_UNIT: &str = "proxy-read-streak";
const STREAK_LIMIT: &str = "proxy_read_streak";
const TAIL_BYTES: u64 = 8 * 1024 * 1024;
const SUBAGENTS: [&str; 2] = ["Explore", "Plan"];
const CARGO_BUILDS: [&str; 8] = ["build", "b", "test", "t", "clippy", "check", "c", "nextest"];
const CARGO_VALUED: &[&str] = &["-C", "-Z", "--color", "--config", "--manifest-path"];
const GIT_VALUED: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path", "--config-env"];
const WRAPPERS: [&str; 6] = ["bash", "sh", "zsh", "north", "nix", "ssh"];
const MAX_DEPTH: usize = 3;
const ROUTING: [&str; 3] = ["fleet", "threads", "gate"];
const SEPARATORS: [char; 9] = ['\n', ';', '&', '|', '(', ')', '`', '{', '}'];
const CHILD_SESSION_ENV: &str = "CLAUDE_CODE_CHILD_SESSION";
const CLAUDE_PID_ENV: &str = "CLAUDE_PID";
const CONFIG_ENV: &str = "CLAUDE_CONFIG_DIR";
const LAND_ACTION: &str = "land through safe-push";
const COMMIT_ACTION: &str = "make a git commit";
const LANDING: [&str; 2] = [LAND_ACTION, COMMIT_ACTION];

pub struct Registered {
    pub conversation: Option<String>,
    pub account: Option<String>,
}

pub struct ClaudeStart {
    pub nested: bool,
    pub print: bool,
    pub conversation: Option<String>,
}

pub trait World {
    fn var(&self, name: &str) -> Option<String>;
    fn registered_proxy(&self) -> Option<Registered>;
    fn profile(&self, account: &str) -> Option<String>;
    fn claude_start(&self) -> Option<ClaudeStart>;
    fn lane_files(&self, cwd: &str) -> Option<Vec<String>>;
}

pub struct Live;

impl World for Live {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }

    fn registered_proxy(&self) -> Option<Registered> {
        let dir = self.var("NORTH_FLEET_DIR").map(PathBuf::from).or_else(|| self.var("HOME").map(|home| PathBuf::from(home).join(".local/state/north/fleet")))?;
        let record: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(PROXY).join("session.json")).ok()?).ok()?;
        let proxy = match record["role"].as_str() {
            Some(role) => role == PROXY,
            None => record["parent"].is_null() && record["task_ref"].is_null(),
        };
        let text = |key: &str| record[key].as_str().map(str::to_owned);
        proxy.then(|| Registered { conversation: text("conversation"), account: text("account") })
    }

    fn profile(&self, account: &str) -> Option<String> {
        crate::guard::host::account_home("claude", account).map(|home| home.display().to_string())
    }

    fn claude_start(&self) -> Option<ClaudeStart> {
        let process = PathBuf::from("/proc").join(self.var(CLAUDE_PID_ENV)?);
        let environ = std::fs::read(process.join("environ")).ok()?;
        let cmdline = std::fs::read(process.join("cmdline")).ok()?;
        let nested = environ.split(|byte| *byte == 0).any(|entry| entry.starts_with(format!("{CHILD_SESSION_ENV}=").as_bytes()));
        let print = cmdline.split(|byte| *byte == 0).skip(1).any(|argument| argument == b"-p" || argument == b"--print");
        let record = own_profile(self).and_then(|profile| std::fs::read(PathBuf::from(profile).join("sessions").join(format!("{}.json", self.var(CLAUDE_PID_ENV)?))).ok());
        let conversation = record.and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).and_then(|record| record["sessionId"].as_str().map(str::to_owned));
        Some(ClaudeStart { nested, print, conversation })
    }

    fn lane_files(&self, cwd: &str) -> Option<Vec<String>> {
        lane_changes(cwd)
    }
}

pub(super) fn lane_changes(cwd: &str) -> Option<Vec<String>> {
    let git = |args: &[&str]| -> Option<Vec<String>> {
        let output = std::process::Command::new("git").arg("-C").arg(cwd).args(args).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
    };
    let base = git(&["merge-base", "HEAD", "origin/main"])?.into_iter().next()?;
    let mut files = git(&["diff", "--name-only", &base])?;
    files.extend(git(&["ls-files", "--others", "--exclude-standard"])?);
    Some(files)
}

pub fn evaluate(input: &HookInput) -> Verdict {
    decide(&Live, input)
}

pub fn decide(world: &impl World, input: &HookInput) -> Verdict {
    if !is_proxy(world, input) {
        return Verdict::Allow;
    }
    refusal(world, input).map_or(Verdict::Allow, |action| Verdict::Deny(message(&action)))
}

pub fn is_proxy(world: &impl World, input: &HookInput) -> bool {
    if let Some(agent) = input.data["agent_type"].as_str() {
        return agent == PROXY;
    }
    if input.data.get("agent_id").is_some() {
        return false;
    }
    let start = world.claude_start();
    if start.as_ref().is_some_and(|start| start.nested) {
        return false;
    }
    if let Some(role) = world.var(ROLE_ENV) {
        return role == PROXY;
    }
    if world.var(SESSION_ENV).is_some() {
        return false;
    }
    let Some(registered) = world.registered_proxy() else { return false };
    if input.data["session_id"].as_str().is_some_and(|session| registered.conversation.as_deref() == Some(session)) {
        return true;
    }
    start.is_some_and(|start| !start.print) && registered.account.and_then(|account| world.profile(&account)).is_some_and(|profile| own_profile(world).is_some_and(|own| normpath(&own) == normpath(&profile)))
}

pub fn is_tom_window(world: &impl World, input: &HookInput) -> bool {
    if is_proxy(world, input) {
        return true;
    }
    if input.data.get("agent_type").is_some() || input.data.get("agent_id").is_some() || world.var(ROLE_ENV).is_some() || world.var(SESSION_ENV).is_some() {
        return false;
    }
    world.claude_start().is_some_and(|start| !start.nested && !start.print)
}

fn own_profile(world: &impl World) -> Option<String> {
    world.var(CONFIG_ENV).or_else(|| world.var("HOME").map(|home| format!("{home}/.claude")))
}

fn message(action: &str) -> String {
    format!("BLOCKED ({UNIT}): the proxy routes and judges and does no implementation itself, so it does not {action}. Staff the work instead: `north fleet launch --task REPO#N ...` (workers skill), then judge the report. Reads, `north fleet|threads|gate`, `gh` reads and Explore/Plan subagents stay open.")
}

fn refusal(world: &impl World, input: &HookInput) -> Option<String> {
    match input.tool_name() {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => edit_under_code(world, input),
        "apply_patch" => Some("apply a patch".into()),
        "Agent" | "spawn_agent" => {
            let kind = input.tool_input["subagent_type"].as_str().unwrap_or("general-purpose");
            (!SUBAGENTS.contains(&kind)).then(|| format!("spawn a `{kind}` subagent (only Explore and Plan are open to it)"))
        }
        name if name.ends_with("spawn_agent") => Some("spawn a native subagent".into()),
        _ if input.is_shell() => input.command().and_then(|command| shell_refusal(world, input, &command.lossy())),
        _ => None,
    }
}

fn edit_under_code(world: &impl World, input: &HookInput) -> Option<String> {
    let home = world.var("HOME")?;
    let path = input.tool_input["file_path"].as_str().or_else(|| input.tool_input["notebook_path"].as_str())?;
    let path = expanduser(path, &home);
    let path = normpath(&join(input.cwd().unwrap_or(&home), &path));
    let code = format!("{home}/code");
    (path == code || path.starts_with(&format!("{code}/"))).then(|| format!("edit {path}"))
}

fn shell_refusal(world: &impl World, input: &HookInput, command: &str) -> Option<String> {
    let first = shell_action(command, false)?;
    if LANDING.contains(&first.as_str()) && lane_is_one_doc_or_config(world, input, command) {
        return shell_action(command, true);
    }
    Some(first)
}

fn lane_is_one_doc_or_config(world: &impl World, input: &HookInput, command: &str) -> bool {
    let Some(dir) = input.cwd().and_then(|cwd| acting_dir(command, cwd, world.var("HOME").as_deref())) else { return false };
    world.lane_files(&dir).is_some_and(|files| matches!(files.as_slice(), [file] if is_doc_or_config(file)))
}

fn acting_dir(command: &str, cwd: &str, home: Option<&str>) -> Option<String> {
    let command = strip_heredocs(command);
    let (base, rest) = last_cd(&command, cwd, home)?;
    let words: Vec<&str> = rest.split(|c: char| c.is_whitespace() || SEPARATORS.contains(&c)).filter(|word| !word.is_empty()).collect();
    if words.iter().any(|word| matches!(*word, "cd" | "pushd" | "popd") || word.starts_with("--git-dir") || word.starts_with("--work-tree")) {
        return None;
    }
    let mut dirs: Vec<String> = Vec::new();
    for pair in words.windows(2).filter(|pair| pair[0] == "-C") {
        let dir = resolve_dir(&base, pair[1], home)?;
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    match dirs.as_slice() {
        [] => Some(base),
        [dir] => Some(dir.clone()),
        _ => None,
    }
}

fn last_cd(command: &str, cwd: &str, home: Option<&str>) -> Option<(String, String)> {
    let mut base = cwd.to_owned();
    let mut landed = false;
    let mut rest: Vec<&str> = Vec::new();
    for segment in command.split(['\n', ';']).flat_map(|part| part.split("&&")) {
        if let ["cd", dir] = segment.split_whitespace().collect::<Vec<_>>().as_slice() {
            if landed {
                return None;
            }
            base = resolve_dir(&base, dir, home)?;
            rest.clear();
            continue;
        }
        landed |= shell_action(segment, false).is_some_and(|action| LANDING.contains(&action.as_str()));
        rest.push(segment);
    }
    Some((base, rest.join("\n")))
}

fn resolve_dir(base: &str, word: &str, home: Option<&str>) -> Option<String> {
    if word.is_empty() || word.contains(['"', '\'', '$', '`', '*', '?', '\\', '(', ')', '{', '}', '[', ']']) {
        return None;
    }
    let word = match (word.starts_with('~'), home) {
        (false, _) => word.to_owned(),
        (true, Some(home)) => expanduser(word, home),
        (true, None) => return None,
    };
    Some(normpath(&join(base, &word)))
}

fn is_doc_or_config(file: &str) -> bool {
    if file.split('/').any(|part| part == "..") {
        return false;
    }
    if let Some(rest) = file.strip_prefix("harness/") {
        if rest.ends_with(".toml") && !rest.contains('/') {
            return true;
        }
        if let Some(skill) = rest.strip_prefix("skills/") {
            return skill.split_once('/').is_some_and(|(_, tail)| tail == "SKILL.md" || tail.starts_with("references/"));
        }
    }
    file.starts_with("docs/")
}

pub fn shell_action(command: &str, lane_ok: bool) -> Option<String> {
    action_in(&strip_heredocs(command), 0, lane_ok)
}

fn action_in(text: &str, depth: usize, lane_ok: bool) -> Option<String> {
    let raw: Vec<char> = text.chars().collect();
    let blanked: Vec<char> = blank_quotes(text).chars().collect();
    if raw.len() != blanked.len() {
        return None;
    }
    let mut start = 0;
    for end in 0..=raw.len() {
        if end < raw.len() && !SEPARATORS.contains(&blanked[end]) {
            continue;
        }
        let words: String = blanked[start..end].iter().collect();
        let words: Vec<&str> = words.split_whitespace().collect();
        if let Some(action) = command_action(&words, depth, lane_ok) {
            return Some(action);
        }
        if depth < MAX_DEPTH && wrapped(&words) {
            let quoted = quoted_text(&raw[start..end], &blanked[start..end]);
            if let Some(action) = action_in(&quoted, depth + 1, lane_ok) {
                return Some(action);
            }
        }
        start = end + 1;
    }
    None
}

fn quoted_text(raw: &[char], blanked: &[char]) -> String {
    raw.iter().zip(blanked).map(|(raw, blank)| if raw != blank && *raw != '"' && *raw != '\'' { *raw } else if raw != blank { '\n' } else { ' ' }).collect()
}

fn command_action(words: &[&str], depth: usize, lane_ok: bool) -> Option<String> {
    let mut at = 0;
    while let Some(word) = words.get(at) {
        if is_assignment(word) {
            at += 1;
            continue;
        }
        match basename(word) {
            "sudo" | "doas" | "exec" | "command" | "builtin" | "nohup" | "setsid" | "nice" | "time" | "then" | "do" | "else" | "!" => at += 1,
            "env" => {
                at += 1;
                while words.get(at).is_some_and(|word| word.starts_with('-') || is_assignment(word)) {
                    at += if matches!(words[at], "-u" | "-C" | "-S") { 2 } else { 1 };
                }
            }
            "timeout" => {
                at += 1;
                while words.get(at).is_some_and(|word| word.starts_with('-')) {
                    at += if matches!(words[at], "-s" | "-k" | "--signal" | "--kill-after") { 2 } else { 1 };
                }
                at += 1;
            }
            _ => break,
        }
    }
    let words = words.get(at..)?;
    let head = basename(words.first()?);
    match head {
        "safe-push" => (!lane_ok).then(|| LAND_ACTION.to_owned()),
        "git" => (!lane_ok && subcommand(&words[1..], GIT_VALUED) == Some("commit") && !words.contains(&"--dry-run")).then(|| COMMIT_ACTION.to_owned()),
        "cargo" => subcommand(&words[1..], CARGO_VALUED).filter(|verb| CARGO_BUILDS.contains(verb)).map(|verb| format!("run cargo {verb}")),
        "cargo-nextest" => Some("run cargo nextest".to_owned()),
        "north" if words.get(1).is_some_and(|verb| ROUTING.contains(verb)) => None,
        _ if depth < MAX_DEPTH && WRAPPERS.contains(&head) => {
            let tail = words.iter().position(|word| matches!(*word, "--" | "-c" | "--command"))?;
            command_action(&words[tail + 1..], depth + 1, lane_ok)
        }
        _ => None,
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn subcommand<'a>(words: &[&'a str], valued: &[&str]) -> Option<&'a str> {
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match *word {
            word if valued.contains(&word) => {
                rest.next();
            }
            word if word.starts_with('-') || word.starts_with('+') => {}
            word => return Some(word),
        }
    }
    None
}

fn wrapped(words: &[&str]) -> bool {
    let Some(at) = words.iter().position(|word| !is_assignment(word)) else { return false };
    let head = basename(words[at]);
    WRAPPERS.contains(&head) && !(head == "north" && words.get(at + 1).is_some_and(|verb| ROUTING.contains(verb)))
}

pub fn evaluate_streak(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") || !is_proxy(&Live, input) || crate::guard::break_glass::lifted_now() {
        return Verdict::Allow;
    }
    let Some(transcript) = input.data["transcript_path"].as_str().and_then(transcript_tail) else { return Verdict::Allow };
    let Ok(hook) = serde_json::from_slice::<serde_json::Value>(&input.raw) else { return Verdict::Allow };
    host::read_streak(&transcript, &hook).map_or(Verdict::Allow, |streak| streak_verdict(&streak, streak_limit()))
}

pub fn streak_limit() -> usize {
    north_base::platform::harness::default_number(STREAK_LIMIT) as usize
}

pub fn streak_verdict(streak: &ReadStreak, limit: usize) -> Verdict {
    if streak.calls <= limit {
        return Verdict::Allow;
    }
    let subject = if streak.subject.is_empty() { "this turn's question" } else { streak.subject.as_str() };
    let read: Vec<String> = streak.shapes.iter().map(|(shape, calls)| format!("`{shape}` x{calls}")).collect();
    Verdict::Deny(format!(
        "STOPPED ({STREAK_UNIT}): this proxy turn made {} read-only calls on \"{subject}\" without a launch, a send or a gate, over the limit of {limit} ({STREAK_LIMIT} in harness/defaults.toml). The proxy routes and judges, so this investigation is a worker's: write the brief with `north work brief new --category debugging-unknown-cause`, its `Item:` line naming \"{subject}\" and what these calls read ({}), staff it with `north fleet launch`, and judge the report.",
        streak.calls,
        read.join(", "),
    ))
}

fn transcript_tail(path: &str) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let whole = if start == 0 { &text[..] } else { text.split_once('\n').map_or("", |(_, rest)| rest) };
    Some(whole.to_owned())
}
