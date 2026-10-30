use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{Value, json};

use crate::pace::target::policy_path;
use crate::platform::{host, process};

const AGENTS: [&str; 2] = ["claude", "codex"];
const NON_RESIDENT_ARGS: [&str; 7] = ["-p", "--print", "--cloud", "exec", "app-server", "mcp-server", "cloud"];
const CLAUDE_ONE_SHOT: [&str; 7] = ["-p", "--print", "--cloud", "-v", "--version", "-h", "--help"];
const CLAUDE_SUBCOMMANDS: [&str; 10] =
    ["auth", "mcp", "plugin", "doctor", "update", "install", "config", "setup-token", "migrate-installer", "agents"];
const CODEX_VALUE_OPTIONS: [&str; 19] = [
    "-c", "--config", "--enable", "--disable", "--remote-auth-token-env", "-i", "--image", "-m", "--model",
    "--local-provider", "-p", "--profile", "-s", "--sandbox", "-C", "--cd", "--add-dir", "-a",
    "--ask-for-approval",
];
const CODEX_SUBCOMMANDS: [&str; 27] = [
    "exec", "e", "review", "login", "logout", "mcp", "plugin", "app-server", "remote-control", "completion",
    "update", "doctor", "sandbox", "debug", "apply", "a", "queue", "archive", "delete", "migrate-rollouts",
    "unarchive", "cloud", "exec-server", "features", "help", "agents", "mcp-server",
];
const GATE_USAGE: &str = "  north resident-cap gate claude|codex [ARG...]   exit 3 with the Hold line when this launch is refused";
const STATUS_USAGE: &str = "  north resident-cap status                        print this host's live count and cap";
const MINIMUM_FACT: usize = 15;

pub(crate) struct Process {
    comm: String,
    ppid: i32,
    argv: Vec<String>,
}

pub(crate) type Table = BTreeMap<i32, Process>;

pub(crate) fn run(arguments: &[String]) -> u8 {
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status", ..] => {
            println!("{}", status());
            0
        }
        ["gate", provider, rest @ ..] if AGENTS.contains(provider) => match gate(provider, rest) {
            Some(line) => {
                eprintln!("{line}");
                3
            }
            None => 0,
        },
        _ => {
            eprintln!("{GATE_USAGE}\n{STATUS_USAGE}");
            2
        }
    }
}


fn cap(name: &str) -> Option<i64> {
    let text = fs::read_to_string(policy_path()).ok()?;
    let document: toml_edit::DocumentMut = text.parse().ok()?;
    document.get("hosts")?.get(name)?.get("resident_cap")?.as_integer()
}

fn proc_root() -> PathBuf {
    std::env::var_os("RESIDENT_CAP_PROC").map_or_else(|| PathBuf::from("/proc"), PathBuf::from)
}

pub(crate) fn process_table(root: &Path) -> Table {
    let Ok(entries) = fs::read_dir(root) else { return Table::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
            let cmdline = fs::read(entry.path().join("cmdline")).ok()?;
            let (head, tail) = stat.rsplit_once(')')?;
            let comm = head.split_once('(')?.1.to_owned();
            let ppid = tail.split_whitespace().nth(1)?.parse().ok()?;
            let mut argv: Vec<&[u8]> = cmdline.split(|byte| *byte == 0).collect();
            argv.pop();
            let argv = argv.into_iter().map(|word| String::from_utf8_lossy(word).into_owned()).collect();
            Some((pid, Process { comm, ppid, argv }))
        })
        .collect()
}

fn agent_kind(process: &Process) -> Option<&'static str> {
    let first = process.argv.first()?;
    let name = Path::new(first).file_name()?.to_str()?;
    let kind = AGENTS.iter().find(|agent| **agent == name)?;
    let one_shot = process.argv[1..].iter().any(|word| NON_RESIDENT_ARGS.contains(&word.as_str()));
    (process.comm == name && !one_shot).then_some(*kind)
}

fn ancestors(table: &Table, pid: i32) -> Vec<i32> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut parent = table.get(&pid).map(|process| process.ppid);
    while let Some(next) = parent.filter(|next| table.contains_key(next) && !seen.contains(next)) {
        chain.push(next);
        seen.insert(next);
        parent = Some(table[&next].ppid);
    }
    chain
}

fn live_sessions() -> Vec<Value> {
    let north = std::env::var("NORTH_BIN")
        .ok()
        .or_else(|| std::env::current_exe().ok().map(|path| path.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "north".into());
    let listed = process::run_within(&north, &["fleet", "list", "--json"], Duration::from_secs(10))
        .filter(|output| output.status.success());
    listed
        .and_then(|output| serde_json::from_slice::<Vec<Value>>(&output.stdout).ok())
        .unwrap_or_default()
}

fn host_pids(sessions: &[Value], status: impl Fn(&str) -> bool) -> BTreeSet<i64> {
    sessions.iter().filter(|session| session.is_object() && status(session["status"].as_str().unwrap_or_default())).filter_map(|session| session["host_pid"].as_i64()).collect()
}

fn registered_pids(table: &Table, sessions: &[Value]) -> BTreeSet<i32> {
    let hosts = host_pids(sessions, |status| !matches!(status, "exited" | "idle"));
    table
        .iter()
        .filter(|(_, process)| hosts.contains(&i64::from(process.ppid)) && AGENTS.contains(&process.comm.as_str()))
        .map(|(pid, _)| *pid)
        .collect()
}

pub(crate) fn count(table: &Table, launcher: Option<i32>, sessions: &[Value]) -> usize {
    let own: BTreeSet<i32> = launcher
        .map(|pid| ancestors(table, pid))
        .unwrap_or_default()
        .into_iter()
        .take_while(|pid| agent_kind(&table[pid]).is_none())
        .collect();
    let nodes = registered_pids(table, sessions);
    let idle: BTreeSet<i32> = host_pids(sessions, |status| status == "idle").into_iter().filter_map(|pid| i32::try_from(pid).ok()).collect();
    let registered = nodes.difference(&own).count();
    let unregistered = table
        .iter()
        .filter(|(_, process)| agent_kind(process).is_some())
        .filter(|(pid, _)| {
            let chain = ancestors(table, **pid);
            !(nodes.contains(pid)
                || chain.iter().any(|parent| idle.contains(parent))
                || chain.iter().any(|parent| agent_kind(&table[parent]).is_some())
                || chain.iter().any(|parent| nodes.contains(parent)))
        })
        .count();
    registered + unregistered
}

pub(crate) fn resident_launch(provider: &str, args: &[&str]) -> bool {
    if provider == "claude" {
        if args.iter().any(|word| CLAUDE_ONE_SHOT.contains(word)) {
            return false;
        }
        return !args.first().is_some_and(|word| CLAUDE_SUBCOMMANDS.contains(word));
    }
    let mut skip = false;
    for word in args {
        if skip {
            skip = false;
        } else if ["-h", "--help", "-V", "--version"].contains(word) {
            return false;
        } else if CODEX_VALUE_OPTIONS.contains(word) {
            skip = true;
        } else if *word == "--" {
            return true;
        } else if !word.starts_with('-') {
            return !CODEX_SUBCOMMANDS.contains(word);
        }
    }
    true
}

fn urgent_fact(case: &str, fact: &str, texts: &[&str]) -> Option<String> {
    if case.to_uppercase() == "URGENT" && fact.trim().chars().count() >= MINIMUM_FACT {
        return Some(fact.trim().to_owned());
    }
    let pattern = Regex::new(r#"(?mi)^\W*CASE=URGENT\s+FACT=(?:["“”]([^"“”]*)["“”]|['‘’]([^'‘’]*)['‘’])"#).ok()?;
    texts
        .iter()
        .filter_map(|text| pattern.captures(text))
        .filter_map(|found| found.get(1).or_else(|| found.get(2)).map(|fact| fact.as_str().trim().to_owned()))
        .find(|fact| fact.chars().count() >= MINIMUM_FACT)
}

fn log_override(fact: &str) {
    let root = std::env::var_os("CODEX_BEHAVIOR_STATE").map_or_else(
        || PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into())).join("codex-behavior"),
        PathBuf::from,
    );
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let cwd = std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
    let fields = [
        ("at", json!(now)),
        ("gate", json!("resident-cap")),
        ("case", json!("URGENT")),
        ("host", json!(host::name())),
        ("cwd", json!(cwd)),
        ("fact", json!(fact.chars().take(300).collect::<String>())),
    ];
    let entry = fields.iter().map(|(key, value)| format!("\"{key}\": {value}")).collect::<Vec<_>>().join(", ");
    let _ = fs::create_dir_all(&root).and_then(|()| {
        let mut log = fs::OpenOptions::new().create(true).append(true).open(root.join("verify-overrides.jsonl"))?;
        writeln!(log, "{{{entry}}}")
    });
}

fn live_count(launcher: Option<i32>) -> usize {
    count(&process_table(&proc_root()), launcher, &live_sessions())
}

fn this_process() -> Option<i32> {
    i32::try_from(std::process::id()).ok()
}

fn hold(texts: &[&str], launcher: Option<i32>) -> Option<String> {
    let name = host::name();
    let limit = cap(&name)?;
    let live = live_count(launcher);
    if i64::try_from(live).ok()? < limit {
        return None;
    }
    let case = std::env::var("CASE").unwrap_or_default();
    let fact = std::env::var("FACT").unwrap_or_default();
    if let Some(fact) = urgent_fact(&case, &fact, texts) {
        log_override(&fact);
        return None;
    }
    Some(format!("Hold: {name} has {live}/{limit} resident agent CLIs; place this worker on laptop|cloud"))
}

fn gate(provider: &str, args: &[&str]) -> Option<String> {
    if !resident_launch(provider, args) {
        return None;
    }
    let launcher = std::env::var("RESIDENT_CAP_PID").ok().filter(|pid| pid.bytes().all(|byte| byte.is_ascii_digit())).and_then(|pid| pid.parse().ok());
    hold(args, launcher.or_else(this_process))
}

pub(crate) fn status() -> String {
    let name = host::name();
    let live = live_count(this_process());
    match cap(&name) {
        Some(limit) => format!("{name}: {live}/{limit} resident agent CLIs"),
        None => format!("{name}: {live} resident agent CLIs, no cap"),
    }
}

#[cfg(test)]
mod tests;
