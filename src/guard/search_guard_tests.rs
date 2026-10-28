use super::trigger_matrix::exercise;
use super::{corpus_scan, modern_search, resource_safe_search};
use regex::Regex;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::LazyLock;

const MODERN: &[&str] = &[
    "grep -rn foo src",
    "find . -name '*.ts'",
    "rgrep foo src",
    "grep --recursive -il foo src",
];
const MODERN_NEAR: &[&str] = &[
    "grep foo a.ts",
    "git grep foo",
    "find . -maxdepth 1 -name '*.ts'",
    "find . -mtime +3 -delete",
];
const RESOURCE: &[&str] = &[
    "rg TARGET /proc/[0-9]*/cwd",
    "rg TARGET /sys",
    "rg TARGET {home}/code/proj",
];
const RESOURCE_NEAR: &[&str] = &[
    "rg TARGET /proc/1234/status",
    "rg TARGET {cwd}/src",
    "find /proc -maxdepth 1 -type d",
];
const CORPUS: &[&str] = &[
    "rg needle {home}/code/north-data",
    "rg needle {home}/code/north-data/accounts",
    "grep -R needle {home}/code/north-data",
    "find {home}/code/north-data -name rollout-*.jsonl",
];
const CORPUS_NEAR: &[&str] = &[
    "rg needle {home}/code/north-data/accounts/openai/acct/sessions/2026/08/12",
    "rg --max-depth 2 needle {home}/code/north-data",
    "convo -x needle",
];

struct Home {
    _guard: tempfile::TempDir,
    path: String,
}

static HOME: LazyLock<Home> = LazyLock::new(|| {
    let guard = tempfile::tempdir().unwrap();
    let root = guard.path().canonicalize().unwrap();
    for directory in [
        "code/proj/main/src",
        "code/proj/worktrees/lane",
        "code/north-data/accounts/openai/acct/sessions/2026/08/12",
        "code/north-data/accounts/claude/acct/projects/slug",
        "code/north-data/archives/a",
        "code/north-data/threads",
        "code/north-data/scratch-x",
        ".local/state",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    for file in [
        "code/proj/main/src/x.ts",
        "code/proj/main/a.ts",
        "code/proj/main/patterns file",
        "code/proj/main/source file.json",
        "code/north-data/accounts/openai/acct/sessions/2026/08/12/rollout-1.jsonl",
        "code/north-data/accounts/claude/acct/projects/slug/s.jsonl",
        "code/north-data/accounts/top.jsonl",
    ] {
        std::fs::write(root.join(file), "x").unwrap();
    }
    std::os::unix::fs::symlink("../../code/north-data", root.join(".local/state/north")).unwrap();
    Home {
        path: root.to_string_lossy().into_owned(),
        _guard: guard,
    }
});

static ABSENT_PID: LazyLock<String> = LazyLock::new(|| {
    std::fs::read_to_string("/proc/sys/kernel/pid_max").map_or_else(|_| "4194304".into(), |max| max.trim().to_owned())
});

fn on_this_host(text: &str) -> String {
    text.replace("/proc/1234", &format!("/proc/{}", *ABSENT_PID))
        .replace("/run/current-system", "/dev/fd")
}

fn key_on_this_host(key: &Value) -> Value {
    match key {
        Value::String(text) => json!(on_this_host(text)),
        Value::Array(parts) => parts.iter().map(key_on_this_host).collect(),
        other => other.clone(),
    }
}

fn filled(text: &str) -> String {
    on_this_host(
        &text
            .replace("{home}", &HOME.path)
            .replace("{cwd}", &format!("{}/code/proj/main", HOME.path)),
    )
}

static MODERN_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^BLOCKED: `(.*)` is (?:a `find` tree search|a recursive grep); use the modern tool instead: `(.*)`\. (?:fd|rg) skips hidden").unwrap()
});
static CORPUS_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^BLOCKED: `(.*?)` rooted at (.*?) is an unscoped sweep").unwrap()
});
static RESOURCE_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^BLOCKED: `(.*?)` would perform (.*) at (.*?)(, traversing main and every worktree\. |\. Do not use recursive)").unwrap()
});

fn unhome(text: &str) -> String {
    text.replace(HOME.path.as_str(), "{home}")
}

fn key(guard: &str, reason: &str) -> Value {
    let part = |captures: &regex::Captures, at| unhome(&captures[at]);
    match guard {
        "modern" => {
            let c = MODERN_KEY.captures(reason).unwrap();
            json!([part(&c, 1), part(&c, 2)])
        }
        "corpus" => {
            let c = CORPUS_KEY.captures(reason).unwrap();
            let doubled = if reason.contains("You also named") {
                "doubled"
            } else {
                ""
            };
            json!([&c[1], part(&c, 2), doubled])
        }
        _ => {
            let c = RESOURCE_KEY.captures(reason).unwrap();
            let kind = if reason.contains("traversing main") {
                "repository"
            } else {
                "virtual"
            };
            json!([&c[1], &c[2], part(&c, 3), kind])
        }
    }
}

fn observed(guard: &str, command: &str, cwd: &str) -> (&'static str, Value) {
    let home = HOME.path.as_str();
    let denied = |reason: Option<String>| match reason {
        Some(reason) => ("deny", key(guard, &reason)),
        None => ("allow", Value::Null),
    };
    match guard {
        "modern" => match modern_search::decide(command, cwd, home) {
            Some(modern_search::Decision::Rewrite(rewritten)) => {
                ("rewrite", json!(unhome(&rewritten)))
            }
            Some(modern_search::Decision::Deny(reason)) => denied(Some(reason)),
            None => denied(None),
        },
        "corpus" => denied(corpus_scan::refusal(command, cwd, home)),
        _ => denied(resource_safe_search::refusal(command, cwd, home)),
    }
}

fn reason_of(guard: &str, command: &str, cwd: &str) -> String {
    let home = HOME.path.as_str();
    match guard {
        "modern" => match modern_search::decide(command, cwd, home) {
            Some(modern_search::Decision::Deny(reason)) => reason,
            other => panic!("{command}: {other:?}"),
        },
        "corpus" => corpus_scan::refusal(command, cwd, home).unwrap(),
        _ => resource_safe_search::refusal(command, cwd, home).unwrap(),
    }
}

#[test]
fn search_guards_decide_as_the_recorded_shell_guards_did() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/search_guards.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() > 1500, "{} recorded decisions", cases.len());
    for case in cases {
        let (guard, cwd, command) = (
            case["g"].as_str().unwrap(),
            filled(case["w"].as_str().unwrap()),
            filled(case["c"].as_str().unwrap()),
        );
        let (decision, observed_key) = observed(guard, &command, &cwd);
        assert_eq!(
            decision,
            case["d"].as_str().unwrap(),
            "{guard} in {cwd}: {command:?}"
        );
        if decision != "allow" {
            assert_eq!(observed_key, key_on_this_host(&case["k"]), "{guard} in {cwd}: {command:?}");
        }
    }
}

#[test]
fn refusal_texts_match_the_recorded_shell_guards_byte_for_byte() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/search_guards.json")).unwrap();
    let goldens = fixture["goldens"].as_array().unwrap();
    assert_eq!(goldens.len(), 6);
    for golden in goldens {
        let (guard, cwd, command) = (
            golden["g"].as_str().unwrap(),
            filled(golden["w"].as_str().unwrap()),
            filled(golden["c"].as_str().unwrap()),
        );
        assert_eq!(
            reason_of(guard, &command, &cwd),
            filled(golden["text"].as_str().unwrap()),
            "{guard}: {command:?}"
        );
    }
}

fn denies(guard: &'static str) -> impl Fn(&str) -> bool {
    move |command| observed(guard, command, &filled("{cwd}")).0 != "allow"
}

#[test]
fn modern_search_refuses_slow_tree_searches_in_command_position_only() {
    exercise(denies("modern"), MODERN, MODERN_NEAR, false);
}

#[test]
fn resource_safe_search_refuses_virtual_roots_and_repository_containers_in_command_position_only() {
    let triggers: Vec<String> = RESOURCE.iter().map(|t| filled(t)).collect();
    let near: Vec<String> = RESOURCE_NEAR.iter().map(|t| filled(t)).collect();
    exercise(
        denies("resource"),
        &triggers.iter().map(String::as_str).collect::<Vec<_>>(),
        &near.iter().map(String::as_str).collect::<Vec<_>>(),
        false,
    );
}

#[test]
fn corpus_scan_refuses_transcript_sweeps_in_command_position_only() {
    let triggers: Vec<String> = CORPUS.iter().map(|t| filled(t)).collect();
    let near: Vec<String> = CORPUS_NEAR.iter().map(|t| filled(t)).collect();
    exercise(
        denies("corpus"),
        &triggers.iter().map(String::as_str).collect::<Vec<_>>(),
        &near.iter().map(String::as_str).collect::<Vec<_>>(),
        false,
    );
}

#[test]
fn a_named_regular_file_is_never_a_tree_walk() {
    let home = Path::new(&HOME.path);
    assert!(home.join("code/proj/main/a.ts").is_file());
    assert_eq!(
        observed("modern", "grep -r foo a.ts", &filled("{cwd}")).0,
        "allow"
    );
    assert_eq!(
        observed("modern", "grep -r foo a.ts src", &filled("{cwd}")).0,
        "deny"
    );
}

#[test]
fn a_find_depth_beyond_any_integer_width_still_becomes_an_fd_depth() {
    let digits = "1".repeat(43);
    let cwd = filled("{cwd}");
    let reason = reason_of(
        "modern",
        &format!("find . -maxdepth 000{digits} -name x"),
        &cwd,
    );
    assert!(reason.contains(&format!("-d {digits}")), "{reason}");
}

#[test]
fn a_process_link_operand_ending_in_a_newline_is_still_refused() {
    assert_eq!(
        observed("resource", "rg x \"/proc/1/cwd\n\"", &filled("{cwd}")).0,
        "deny"
    );
}
