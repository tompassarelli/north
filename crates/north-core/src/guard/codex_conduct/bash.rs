use super::state::{self, head_commit, py_str, sha256_hex, tree_state, with_shared, with_state};
use super::{Ctx, asked_keyword, gates, pattern};
use crate::hook::{HookInput, Verdict};
use north_base::shell::strip_heredocs;
use serde_json::Value;
use std::path::Path;

const MAX_BOXES: usize = 5;
const RUNNERS: [&str; 22] = [
    "bun", "bunx", "cargo", "npm", "pnpm", "yarn", "npx", "pytest", "go", "make", "just", "nix", "firn", "deno", "uv", "python", "python3", "bash", "sh", "zig", "dotnet", "gradle",
];

pattern!(SEPARATOR, r"&&|\|\||;|\||\n");
pattern!(ASSIGNMENT, r"^[A-Za-z_][A-Za-z0-9_]*=");
pattern!(CAPACITY_WORD, r"machine-capacity(\.mjs)?$");
pattern!(CHECK_WORDS, r"\b(test|tests|check|lint|typecheck|tsc|clippy|validate|verify|soak|parity|bench|perf|smoke|build|farm|sweep)\b");
pattern!(JUSTIFY_PREFIX, r#"^\s*CASE=([A-Za-z])\s+FACT=("([^"]*)"|'([^']*)')\s*"#);
pattern!(LEADING_CD, r"^\s*cd\s+\S+\s*&&\s*");
pattern!(CAPACITY_RUN, r"\bmachine-capacity\S*\s+run\b.*?\s--\s+");
pattern!(WAIT_FLAGS, r"\s--(wait|exit-status)\b|\s--out(=|\s+)\S+");
pattern!(BOXES, r"(?m)^\s*[-*] \[[ xX]\] (.+)$");
pattern!(DONE_WHEN, r"(?i)done when");
pattern!(NOT_REQUIRED, r"(?i)not required");
pattern!(GUARANTEE, r"(?i)\b(every|never|always|guarantee[sd]?|full fidelity|bit[- ]exact|no (recurring|unexplained))\b");
pattern!(HEAVY_SUITE, r"\b(scripts/lua-tests\.ts|scripts/cpuTiers\.ts|scripts/cpuField\.ts)\b");
pattern!(PROCESS_TOOLS, r"\b(pkill|pgrep|kill|ps|grep|rg|cat|sed)\b");
pattern!(WORKER_JOB, r"\bgh run (view|watch)\b.*--log|\b(rg|grep|tail|less|cat)\b[^|;&]*\.log\b|\bwisp (lan|pad|native|capture|soak)\b");
pattern!(ORCH_REASON, r#"\bORCH_RUNS_BECAUSE=("[^"]{8,}"|'[^']{8,}'|\S{8,})"#);
pattern!(RERUN, r"\bgh run rerun\s+(\d+)");
pattern!(MEASURE_CMD, r"(?i)\b(bench\w*|perf|soak|profil\w*|hyperfine)\b|(^|[;&|]\s*)time\s|\b(for|while)\b[^;]*;\s*do\b");

pub(super) fn basename(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

pub(super) fn segments(command: &str) -> Vec<Vec<&str>> {
    SEPARATOR
        .split(command)
        .filter_map(|part| {
            let mut words: Vec<&str> = part.split_whitespace().collect();
            let skip = words.iter().take_while(|word| ASSIGNMENT.is_match(word)).count();
            words.drain(..skip);
            if words.first().is_some_and(|first| ["env", "time", "nice", "exec"].contains(first)) {
                words.remove(0);
            }
            (!words.is_empty()).then_some(words)
        })
        .collect()
}

fn capacity_query(words: &[&str]) -> bool {
    words.iter().take(3).enumerate().any(|(i, word)| {
        CAPACITY_WORD.is_match(word) && words.get(i + 1).is_some_and(|next| ["estimate", "status", "probe", "mode"].contains(next))
    })
}

pub(super) fn is_check(command: &str) -> bool {
    segments(&strip_heredocs(command)).iter().any(|words| {
        if capacity_query(words) {
            return false;
        }
        let runner = basename(words[0]);
        if RUNNERS.contains(&runner) && CHECK_WORDS.is_match(&words[1..].join(" ")) {
            return true;
        }
        (runner.ends_with(".sh") || runner.starts_with("./")) && CHECK_WORDS.is_match(&words.join(" "))
    })
}

pub(super) struct Case<'a> {
    pub letter: Option<String>,
    pub fact: String,
    pub bare: &'a str,
}

pub(super) fn parse_case(command: &str) -> Case<'_> {
    match JUSTIFY_PREFIX.captures(command) {
        Some(m) => Case {
            letter: Some(m[1].to_uppercase()),
            fact: m.get(3).or(m.get(4)).map_or("", |f| f.as_str()).trim().to_owned(),
            bare: &command[m.get(0).map_or(0, |all| all.end())..],
        },
        None => Case { letter: None, fact: String::new(), bare: command },
    }
}

pub(super) fn check_key(cwd: &Value, command: &str) -> String {
    let words = parse_case(command).bare.split_whitespace().collect::<Vec<_>>().join(" ");
    sha256_hex(format!("{}\0{words}", py_str(cwd)).as_bytes())[..24].to_owned()
}

pub(super) fn check_name(command: &str) -> String {
    let command = LEADING_CD.replace(parse_case(command).bare, "");
    let command = CAPACITY_RUN.replace_all(&command, "");
    let command = WAIT_FLAGS.replace_all(&command, "");
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn gh_issue_action(command: &str) -> Option<(String, Vec<String>)> {
    segments(command)
        .into_iter()
        .find(|words| words.len() >= 3 && basename(words[0]) == "gh" && words[1] == "issue")
        .map(|words| (words[2].to_owned(), words.into_iter().map(str::to_owned).collect()))
}

pub(super) fn issue_body(command: &str, cwd: Option<&str>) -> String {
    let Some(tokens) = north_base::shell::shlex_split(command) else {
        return command.to_owned();
    };
    for pair in tokens.windows(2) {
        if pair[0] == "--body" || pair[0] == "-b" {
            return pair[1].clone();
        }
        if pair[0] == "--body-file" || pair[0] == "-F" {
            let path = Path::new(&pair[1]);
            let path = if path.is_absolute() { path.to_path_buf() } else { Path::new(cwd.filter(|c| !c.is_empty()).unwrap_or(".")).join(path) };
            return std::fs::read(path).map_or_else(|_| command.to_owned(), |b| String::from_utf8_lossy(&b).into_owned());
        }
    }
    tokens.iter().find_map(|t| t.strip_prefix("--body=")).map_or_else(|| command.to_owned(), str::to_owned)
}

pub(super) fn issue_shape_problem(body: &str, creating: bool) -> Option<String> {
    let boxes: Vec<&str> = BOXES.captures_iter(body).filter_map(|m| m.get(1)).map(|m| m.as_str()).collect();
    if creating && !DONE_WHEN.is_match(body) {
        return Some("it has no Done when list".into());
    }
    if creating && !NOT_REQUIRED.is_match(body) {
        return Some("it has no Not required list, so scope will grow".into());
    }
    if boxes.len() > MAX_BOXES {
        return Some(format!("it has {} boxes; at most {MAX_BOXES}, each a pass/fail check", boxes.len()));
    }
    boxes.iter().find_map(|line| {
        GUARANTEE.find(line).map(|found| {
            format!(
                "box \"{}\" promises \"{}\", which no finite test can close. Make it a measured check, like \"500 inputs, 0 lost\"",
                super::chars(line, 70),
                found.as_str()
            )
        })
    })
}

pub(super) fn is_measure(command: &str) -> bool {
    MEASURE_CMD.is_match(command)
}

fn heavy_suite(ctx: &Ctx, command: &str, cwd: Option<&str>) -> Option<String> {
    let suite = HEAVY_SUITE.find(command)?.as_str();
    if PROCESS_TOOLS.is_match(command) {
        return None;
    }
    let (pid, started) = state::running_copy(cwd.filter(|c| !c.is_empty())?, suite)?;
    Some(format!(
        "No. `{suite}` is already running in this worktree (pid {pid}, {}). Wait for it or stop it; one copy at a time. Full suites belong on GitHub's runners: `bun wisp farm`.",
        super::minutes(ctx.now - started)
    ))
}

fn issue_create(ctx: &Ctx, event: &Value, command: &str, cwd: Option<&str>) -> Verdict {
    if let Some(problem) = issue_shape_problem(&issue_body(command, cwd), true) {
        return Verdict::Deny(format!("No. This issue can't close: {problem}. Rewrite it and retry."));
    }
    with_state(ctx, &state::agent_key(event), |state| {
        let recent = state::prompts(state, 3);
        let (opened, closed) = state::tree_counts(ctx, event, state);
        if opened > closed && !asked_keyword(&["issue", "ticket"], &recent) {
            return Verdict::Deny(format!(
                "No. This session opened {opened} issues and closed {closed}. Close one before opening another. A problem that blocks nothing goes in your report as one line."
            ));
        }
        Verdict::Allow
    })
}

pub(super) fn pre_bash(ctx: &Ctx, input: &HookInput, event: &Value) -> Verdict {
    let command = super::command(event);
    let cwd = event["cwd"].as_str();
    if let Some(reason) = heavy_suite(ctx, &command, cwd) {
        return Verdict::Deny(reason);
    }
    if gh_issue_action(&command).is_some_and(|(action, _)| action == "create") {
        return issue_create(ctx, event, &command, cwd);
    }
    let spawned = with_state(ctx, &state::agent_key(event), |state| state::count(state.get("spawned")));
    if spawned != 0 && (is_check(&command) || WORKER_JOB.is_match(&strip_heredocs(&command))) && !ORCH_REASON.is_match(&command) {
        return Verdict::Deny(format!(
            "No. You're orchestrating {spawned} workers, so this is a worker's job: tests, builds, native sessions and log digging. Hand it to a worker and go back to assigning, merging and closing. If no worker can run it, prefix the command with ORCH_RUNS_BECAUSE=\"<why>\"."
        ));
    }
    let case = parse_case(&command);
    if let Some(verdict) = gates::pre_justify(ctx, input, event, &command) {
        return verdict;
    }
    if let Some(run) = RERUN.captures(case.bare)
        && state::run_conclusion(&run[1]) == "success"
    {
        return gates::rerun_gate(ctx, input, event, &format!("run {}", &run[1]), "an earlier run", &format!("gh-run\0{}", &run[1]), &case, &command);
    }
    if is_measure(case.bare)
        && let Some(verdict) = gates::measure_gate(ctx, input, event, &case)
    {
        return verdict;
    }
    if !is_check(case.bare) {
        return Verdict::Allow;
    }
    rerun_check(ctx, input, event, &case, &command)
}

fn rerun_check(ctx: &Ctx, input: &HookInput, event: &Value, case: &Case, command: &str) -> Verdict {
    let cwd = event["cwd"].as_str().unwrap_or("None");
    let name = check_name(case.bare);
    if let Some(commit) = head_commit(cwd) {
        let fingerprint = format!("{commit}\0{name}");
        let record = with_shared(ctx, |shared| shared.get(&fingerprint).cloned());
        if let Some(record) = record.filter(|r| state::truthy(Some(r))) {
            let at = super::clock(record["at"].as_f64().unwrap_or(0.0));
            return gates::rerun_gate(ctx, input, event, &name, &at, &fingerprint, case, command);
        }
    }
    let key = check_key(&event["cwd"], case.bare);
    let record = with_state(ctx, &state::agent_key(event), |state| state["passed"].get(&key).cloned());
    let Some(record) = record.filter(|r| state::truthy(Some(r))) else {
        return Verdict::Allow;
    };
    let tree = tree_state(cwd);
    if tree.as_deref().map_or(Value::Null, Value::from) != record["tree"] {
        return Verdict::Allow;
    }
    let at = super::clock(record["at"].as_f64().unwrap_or(0.0));
    let tree = tree.unwrap_or_else(|| "None".to_owned());
    gates::rerun_gate(ctx, input, event, &name, &at, &format!("{tree}\0{name}"), case, command)
}
