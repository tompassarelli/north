use crate::guard::host::{self, ReadStreak};
use crate::guard::proxy_reach;
use crate::fleet::runtime::{PROXY, ROLE_ENV, SESSION_ENV};
use crate::guard::paths::{basename, expanduser, join, normpath};
use crate::hook_input::{HookInput, Verdict};
use crate::shell::{blank_quotes, strip_heredocs};
use north_base::shell::{is_separator, words};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

const UNIT: &str = "proxy-role-guard";
const REACH_UNIT: &str = "proxy-reach-guard";
const STREAK_UNIT: &str = "proxy-read-streak";
const STREAK_LIMIT: &str = "proxy_read_streak";
const BUDGET_UNIT: &str = "proxy-look-budget";
const LOOK_BUDGET: &str = "proxy_look_budget";
const TAIL_BYTES: u64 = 8 * 1024 * 1024;
const SUBAGENTS: [&str; 2] = ["Explore", "Plan"];
const CARGO_BUILDS: [&str; 11] = ["build", "b", "test", "t", "clippy", "check", "c", "nextest", "run", "bench", "doc"];
const SCRIPT_RUNNERS: [&str; 4] = ["bun", "npm", "pnpm", "yarn"];
const SCRIPT_BUILDS: [&str; 5] = ["test", "build", "check", "lint", "typecheck"];
const MAKERS: [&str; 5] = ["make", "just", "pytest", "tsc", "ninja"];
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
    fn break_glass(&self) -> bool;
    fn status_verb(&self, words: &[&str]) -> bool;
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

    fn break_glass(&self) -> bool {
        crate::guard::break_glass::lifted_now()
    }

    fn status_verb(&self, words: &[&str]) -> bool {
        host::status_verb(words)
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
    if let Some(action) = refusal(world, input) {
        return Verdict::Deny(message(&action));
    }
    match reach(world, input) {
        Some(what) if !world.break_glass() => Verdict::Deny(reach_message(&what)),
        _ => Verdict::Allow,
    }
}

fn reach(world: &impl World, input: &HookInput) -> Option<String> {
    let home = world.var("HOME")?;
    let status_verb = |words: &[&str]| world.status_verb(words);
    let look = proxy_reach::Look { home: &home, cwd: input.cwd().unwrap_or(&home), status_verb: &status_verb };
    proxy_reach::diagnosis(&look, input.tool_name(), &input.tool_input)
}

fn reach_message(what: &str) -> String {
    format!("BLOCKED ({REACH_UNIT}): the proxy diagnoses nothing outside break-glass (proxy_role in harness/defaults.toml), so it does not {what}. Hand the cause to a worker: write the brief with `north work brief new --category debugging-unknown-cause --item REPO#N`, which prints the header lines a brief needs (`Item:`, `Category:`, `Spec:`, `Scope:`, `Surface:`, `Verify:`, `Done when:`, `ETA:`, `Files:`), name in it what this call would have read, staff it with `north fleet launch --task REPO#N`, and judge the report. Launches, sends, gates, issue reads and edits, reports, the north status verbs, the proxy's scratch and `git log --oneline origin/main` stay open; only `north fleet break-glass`, while workers cannot launch, lifts this.")
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
    if redirects_into_repository(words) {
        return Some("write a repository file with a shell redirect".to_owned());
    }
    match head {
        "tee" if words.iter().any(|word| in_repository(word)) => Some("write a repository file with tee".to_owned()),
        "patch" => Some("apply a patch".to_owned()),
        "git" if subcommand(&words[1..], GIT_VALUED) == Some("apply") && !words.contains(&"--check") => Some("apply a patch".to_owned()),
        _ if MAKERS.contains(&head) => Some(format!("run {head}")),
        "go" if matches!(subcommand(&words[1..], &[]), Some("build" | "test" | "vet")) => Some("run go build or test".to_owned()),
        "nix" if matches!(subcommand(&words[1..], &[]), Some("build")) || words.windows(2).any(|pair| pair == ["flake", "check"]) => Some("run a nix build or check".to_owned()),
        _ if SCRIPT_RUNNERS.contains(&head) && script_builds(&words[1..]) => Some(format!("run {head} build or test")),
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

fn in_repository(word: &str) -> bool {
    word.contains("/code/")
}

fn redirects_into_repository(words: &[&str]) -> bool {
    words.iter().enumerate().any(|(at, word)| {
        let Some(target) = word.trim_start_matches(|c: char| c.is_ascii_digit()).strip_prefix('>') else { return false };
        let target = target.trim_start_matches('>');
        if target.starts_with('&') {
            return false;
        }
        in_repository(if target.is_empty() { words.get(at + 1).copied().unwrap_or_default() } else { target })
    })
}

fn script_builds(words: &[&str]) -> bool {
    let word = match subcommand(words, &[]) {
        Some("run" | "x") => words.iter().skip_while(|word| !matches!(**word, "run" | "x")).nth(1).copied(),
        word => word,
    };
    word.is_some_and(|word| SCRIPT_BUILDS.contains(&word) || word.starts_with("build"))
}

fn wrapped(words: &[&str]) -> bool {
    let Some(at) = words.iter().position(|word| !is_assignment(word)) else { return false };
    let head = basename(words[at]);
    WRAPPERS.contains(&head) && !(head == "north" && words.get(at + 1).is_some_and(|verb| ROUTING.contains(verb)))
}

pub fn evaluate_reads(input: &HookInput) -> Verdict {
    reads_verdict(input)
}

pub fn advise_reads(input: &HookInput) -> Verdict {
    match reads_verdict(input) {
        Verdict::Deny(reason) => Verdict::Advise(reason),
        verdict => verdict,
    }
}

fn reads_verdict(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") || !is_proxy(&Live, input) || crate::guard::break_glass::lifted_now() {
        return Verdict::Allow;
    }
    let Some(transcript) = input.data["transcript_path"].as_str().and_then(transcript_tail) else { return Verdict::Allow };
    let Ok(hook) = serde_json::from_slice::<serde_json::Value>(&input.raw) else { return Verdict::Allow };
    host::read_streak(&transcript, &hook).map_or(Verdict::Allow, |streak| match budget_verdict(&streak, &budgets()) {
        Verdict::Allow => streak_verdict(&streak, streak_limit()),
        refused => refused,
    })
}

pub fn spends_no_look(tool: &str, input: &Value) -> bool {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let (file, command) = (text("file_path"), text("command"));
    var("HOME").is_some_and(|home| harness_load(&home, var(CONFIG_ENV).as_deref(), tool, if tool == "Read" { file } else { command }))
}

pub fn harness_load(home: &str, config: Option<&str>, tool: &str, text: &str) -> bool {
    match tool {
        "Skill" => true,
        "Read" => {
            let path = normpath(&join(home, &expanduser(text, home)));
            let profiles = format!("{home}/.local/state/north/profiles/");
            config.is_some_and(|config| path.starts_with(&format!("{}/skills/", config.trim_end_matches('/')))) || path.starts_with(&format!("{home}/.claude/skills/")) || (path.starts_with(&profiles) && path.contains("/skills/"))
        }
        "Bash" => {
            let Some(words) = words(text) else { return false };
            let words: Vec<&str> = words.iter().map(String::as_str).collect();
            !words.iter().any(|word| is_separator(word)) && matches!(words.as_slice(), [north, rest @ ..] if basename(north) == "north" && (rest.contains(&"--help") || matches!(rest, ["work", "brief", "new", ..])))
        }
        _ => false,
    }
}

pub struct Budgets {
    pub looks: usize,
}

pub fn budgets() -> Budgets {
    let number = |name| north_base::platform::harness::default_number(name) as usize;
    Budgets { looks: number(LOOK_BUDGET) }
}

pub fn budget_verdict(streak: &ReadStreak, budgets: &Budgets) -> Verdict {
    let message = streak.message;
    if !message.looking || message.looks <= budgets.looks {
        return Verdict::Allow;
    }
    let subject = if streak.subject.is_empty() { "this message" } else { streak.subject.as_str() };
    let read: Vec<String> = streak.shapes.iter().map(|(shape, calls)| format!("`{shape}` x{calls}")).collect();
    Verdict::Deny(format!(
        "BLOCKED ({BUDGET_UNIT}): read-only calls are refused from here and your turn goes on: this message from Tom has had {} read-only calls on \"{subject}\", over the budget of {} ({LOOK_BUDGET} in harness/defaults.toml); a launch, send or gate does not refill it, only Tom's next message does. Launches, sends, gates, `north work brief new`, skill loads and `north ... --help` stay open. The proxy routes and judges, so finding this cause is a worker's: write the brief with `north work brief new --category debugging-unknown-cause`, its `Item:` line naming \"{subject}\" and what these calls read ({}), staff it with `north fleet launch`, and judge the report.",
        message.looks,
        budgets.looks,
        read.join(", "),
    ))
}

pub fn streak_limit() -> usize {
    north_base::platform::harness::default_number(STREAK_LIMIT) as usize
}

pub fn streak_verdict(streak: &ReadStreak, limit: usize) -> Verdict {
    if !streak.message.looking || streak.calls <= limit {
        return Verdict::Allow;
    }
    let subject = if streak.subject.is_empty() { "this turn's question" } else { streak.subject.as_str() };
    let read: Vec<String> = streak.shapes.iter().map(|(shape, calls)| format!("`{shape}` x{calls}")).collect();
    Verdict::Deny(format!(
        "BLOCKED ({STREAK_UNIT}): read-only calls are refused from here and your turn goes on: this proxy turn made {} read-only calls on \"{subject}\" without a launch, a send or a gate, over the limit of {limit} ({STREAK_LIMIT} in harness/defaults.toml). Launches, sends, gates, `north work brief new`, skill loads and `north ... --help` stay open. The proxy routes and judges, so this investigation is a worker's: write the brief with `north work brief new --category debugging-unknown-cause`, its `Item:` line naming \"{subject}\" and what these calls read ({}), staff it with `north fleet launch`, and judge the report.",
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
