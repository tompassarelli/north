use crate::fleet::runtime::{PROXY, ROLE_ENV, SESSION_ENV};
use crate::guard::paths::{basename, expanduser, join, normpath};
use crate::hook::{HookInput, Verdict};
use crate::platform::process::run_command_within;
use crate::shell::{blank_quotes, strip_heredocs};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

const UNIT: &str = "proxy-role-guard";
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

pub struct Registered {
    pub conversation: Option<String>,
    pub account: Option<String>,
}

pub struct ClaudeStart {
    pub nested: bool,
    pub print: bool,
}

pub trait World {
    fn var(&self, name: &str) -> Option<String>;
    fn registered_proxy(&self) -> Option<Registered>;
    fn profile(&self, account: &str) -> Option<String>;
    fn claude_start(&self) -> Option<ClaudeStart>;
    fn fleet(&self) -> Option<String>;
    fn ready(&self) -> Option<String>;
    fn seen(&self, session: &str, items: &str) -> bool;
}

pub struct Live;

fn north() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"))
}

fn stdout_of(arguments: &[&str], limit: Duration) -> Option<String> {
    let mut command = Command::new(north());
    command.args(arguments);
    let output = run_command_within(command, None, limit)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

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
        Some(ClaudeStart { nested, print })
    }

    fn fleet(&self) -> Option<String> {
        stdout_of(&["fleet", "list", "--json"], Duration::from_secs(5))
    }

    fn ready(&self) -> Option<String> {
        stdout_of(&["threads", "ready"], Duration::from_secs(25))
    }

    fn seen(&self, session: &str, items: &str) -> bool {
        let Some(dir) = self.var("HOME").map(|home| PathBuf::from(home).join(".local/state/north/proxy-idle")) else { return false };
        let file = dir.join(session.replace('/', "_"));
        if std::fs::read_to_string(&file).is_ok_and(|seen| seen == items) {
            return true;
        }
        let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, items));
        false
    }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(&Live, input)
}

pub fn decide(world: &impl World, input: &HookInput) -> Verdict {
    if !is_proxy(world, input) {
        return Verdict::Allow;
    }
    match input.data["hook_event_name"].as_str() {
        Some("Stop") => idle_fleet(world, input).map_or(Verdict::Allow, Verdict::Deny),
        _ => refusal(world, input).map_or(Verdict::Allow, |action| Verdict::Deny(message(&action))),
    }
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
        _ if input.is_shell() => input.command().and_then(|command| shell_action(&command.lossy())),
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

pub fn shell_action(command: &str) -> Option<String> {
    action_in(&strip_heredocs(command), 0)
}

fn action_in(text: &str, depth: usize) -> Option<String> {
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
        if let Some(action) = command_action(&words, depth) {
            return Some(action);
        }
        if depth < MAX_DEPTH && wrapped(&words) {
            let quoted = quoted_text(&raw[start..end], &blanked[start..end]);
            if let Some(action) = action_in(&quoted, depth + 1) {
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

fn command_action(words: &[&str], depth: usize) -> Option<String> {
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
        "safe-push" => Some("land through safe-push".to_owned()),
        "git" => (subcommand(&words[1..], GIT_VALUED) == Some("commit")).then(|| "make a git commit".to_owned()),
        "cargo" => subcommand(&words[1..], CARGO_VALUED).filter(|verb| CARGO_BUILDS.contains(verb)).map(|verb| format!("run cargo {verb}")),
        "cargo-nextest" => Some("run cargo nextest".to_owned()),
        "north" if words.get(1).is_some_and(|verb| ROUTING.contains(verb)) => None,
        _ if depth < MAX_DEPTH && WRAPPERS.contains(&head) => {
            let tail = words.iter().position(|word| matches!(*word, "--" | "-c" | "--command"))?;
            command_action(&words[tail + 1..], depth + 1)
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

fn idle_fleet(world: &impl World, input: &HookInput) -> Option<String> {
    if input.data["stop_hook_active"].as_bool() == Some(true) {
        return None;
    }
    let own = input.data["session_id"].as_str().unwrap_or_default();
    let sessions: Value = serde_json::from_str(&world.fleet()?).ok()?;
    let working = sessions.as_array()?.iter().any(|session| session["status"] == "working" && session["conversation"].as_str() != Some(own) && session["name"] != PROXY);
    if working {
        return None;
    }
    let ready = world.ready()?;
    let items: Vec<&str> = ready.lines().filter_map(|line| line.split('\t').next()).filter(|item| item.contains('#') && !item.contains(' ')).collect();
    if items.is_empty() || world.seen(own, &items.join("\n")) {
        return None;
    }
    let sample = items.iter().take(5).copied().collect::<Vec<_>>().join(", ");
    Some(format!(
        "{UNIT}: {} ready items ({sample}{}) and no fleet session is working. Staff them now with `north fleet launch --task REPO#N ...` up to the free worker slots before ending the turn, or tell Tom what blocks staffing.",
        items.len(),
        if items.len() > 5 { ", ..." } else { "" }
    ))
}

fn wrapped(words: &[&str]) -> bool {
    let Some(at) = words.iter().position(|word| !is_assignment(word)) else { return false };
    let head = basename(words[at]);
    WRAPPERS.contains(&head) && !(head == "north" && words.get(at + 1).is_some_and(|verb| ROUTING.contains(verb)))
}
