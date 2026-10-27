use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use crate::foundation::clock::Timestamp;

use crate::platform::git::run as git;
fn text(output: std::io::Result<Output>) -> Result<String, String> {
    output
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .map_err(|e| e.to_string())
}
fn success(output: std::io::Result<Output>) -> Result<bool, String> {
    output
        .map(|out| out.status.success())
        .map_err(|e| e.to_string())
}
fn origin_owner(repo: &Path) -> Result<String, String> {
    let url = text(git(repo, &["remote", "get-url", "origin"]))?;
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let mut parts = url.rsplit(['/', ':']);
    parts.next();
    parts
        .next()
        .filter(|owner| !owner.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("origin remote {url} names no owner"))
}
fn issue_closed(repo: &Path, owner: Option<&str>, name: &str, number: &str) -> Result<bool, String> {
    let owner = match owner {
        Some(owner) => owner.to_owned(),
        None => origin_owner(repo)?,
    };
    let out = Command::new("gh")
        .current_dir(repo)
        .args(["issue", "view", number, "--repo", &format!("{owner}/{name}")])
        .args(["--json", "state", "--jq", ".state"])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "CLOSED")
}
enum Predecessor<'a> {
    Issue { owner: Option<&'a str>, name: &'a str, number: &'a str },
    Sha(&'a str),
    Subject(&'a str),
}
fn predecessor(follows: &str) -> Predecessor<'_> {
    static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9a-f]{7,40})(?:\s.*)?$").unwrap());
    static ISSUE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:([\w.-]+)/)?([\w.-]+)#(\d+)(?:\s.*)?$").unwrap());
    if let Some(c) = ISSUE.captures(follows) {
        return Predecessor::Issue {
            owner: c.get(1).map(|m| m.as_str()),
            name: c.get(2).unwrap().as_str(),
            number: c.get(3).unwrap().as_str(),
        };
    }
    if let Some(c) = SHA.captures(follows) {
        return Predecessor::Sha(c.get(1).unwrap().as_str());
    }
    Predecessor::Subject(follows)
}
fn subject_on_main(repo: &Path, subject: &str) -> Result<bool, String> {
    let log = text(git(
        repo,
        &["log", "-n2000", "--format=%B%x00", "refs/remotes/origin/main"],
    ))?;
    Ok(log
        .split('\0')
        .any(|m| m.trim().split('\n').next() == Some(subject)))
}
fn landed(repo: &Path, follows: &str) -> Result<bool, String> {
    match predecessor(follows) {
        Predecessor::Issue { owner, name, number } => issue_closed(repo, owner, name, number),
        Predecessor::Sha(sha) => success(git(
            repo,
            &["merge-base", "--is-ancestor", sha, "refs/remotes/origin/main"],
        )),
        Predecessor::Subject(subject) => subject_on_main(repo, subject),
    }
}
fn item_closed(repo: &Path, item: &str) -> Result<bool, String> {
    match predecessor(item) {
        Predecessor::Issue { owner, name, number } => issue_closed(repo, owner, name, number),
        _ => Ok(false),
    }
}
fn field<'a>(b: &'a Value, key: &str) -> &'a str {
    b[key].as_str().unwrap_or("")
}
fn threads(root: &Path, args: &[&str]) -> std::io::Result<Output> {
    Command::new(root.join("bin/threads"))
        .arg("queue")
        .args(args)
        .output()
}
fn claude_tier(tier: &str) -> bool {
    ["worker", "claude:", "sonnet", "opus", "haiku", "fable"]
        .iter()
        .any(|prefix| tier.starts_with(prefix))
}
fn gate(root: &Path, b: &Value, repo: &Path) -> Result<Option<String>, String> {
    let tier = field(b, "tier");
    let tool = if claude_tier(tier) {
        json!({"tool_name":"Agent","tool_input":{"subagent_type":tier,"description":b["item"],"prompt":b["brief"],"run_in_background":true}})
    } else {
        json!({"tool_name":"spawn_agent","tool_input":{"message":b["brief"]}})
    };
    let mut event = tool;
    event["hook_event_name"] = json!("PreToolUse");
    event["cwd"] = json!(repo);
    let mut child = Command::new("python3")
        .arg(root.join("harness/hooks/lib/spawn_capacity.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let decision: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let decision = &decision["hookSpecificOutput"];
    Ok(if decision["permissionDecision"] == "deny" {
        decision["permissionDecisionReason"]
            .as_str()
            .map(str::to_owned)
    } else {
        None
    })
}
fn start(b: &Value) -> Result<bool, String> {
    let tier = field(b, "tier");
    let route = if claude_tier(tier) {
        " Call Claude Agent with run_in_background: true."
    } else {
        ""
    };
    let message = format!(
        "{}\nStaff this queued box now as a fresh worker at {tier}.{route} If you staff it under a worker of your own, run `north threads queue follow-started {}` so it is not sent again.",
        field(b, "brief"),
        b["id"]
    );
    let Ok(mut child) = Command::new("north")
        .args(["fleet", "send", field(b, "parent"), &message])
        .stdin(Stdio::null())
        .spawn()
    else {
        return Ok(false);
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
            _ => (),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn alert(root: &Path, b: &Value, why: &str, now: f64) -> Result<(), String> {
    if !now.is_finite() {
        return Err("invalid timestamp".into());
    }
    let line = format!(
        "FOLLOWS {} box {} {} (Follows: {}, tier {}): {why}",
        Timestamp::from_unix_seconds_f64(now).iso_minute(),
        b["id"],
        field(b, "item"),
        field(b, "follows"),
        field(b, "tier")
    );
    if !success(threads(
        root,
        &["follow-alert", &b["id"].to_string(), &line],
    ))? {
        return Ok(());
    }
    let _ = Command::new("north")
        .args(["fleet", "send", field(b, "parent"), &line])
        .stdin(Stdio::null())
        .status();
    println!("{line}");
    Ok(())
}
fn execute(only: Option<PathBuf>) -> Result<u8, String> {
    let home = PathBuf::from(env::var_os("HOME").ok_or("HOME is missing")?);
    let state = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"));
    fs::create_dir_all(state.join("agents")).map_err(|e| e.to_string())?;
    let lock = OpenOptions::new()
        .append(true)
        .create(true)
        .open(state.join("agents/follows-release.lock"))
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let root = crate::platform::harness::harness_root()?;
    let listing = threads(&root, &["follow-list"]).map_err(|e| e.to_string())?;
    if !listing.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&listing.stderr).trim());
        return Ok(1);
    }
    let mut boxes: Vec<Value> = serde_json::from_slice(if listing.stdout.is_empty() {
        b"[]"
    } else {
        &listing.stdout
    })
    .map_err(|e| e.to_string())?;
    let repos = env::var_os("FOLLOWS_RELEASE_ROOT")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("code"));
    let name = only
        .as_ref()
        .map(|repo| -> Result<String, String> {
            let url = text(git(repo, &["remote", "get-url", "origin"]))?;
            Ok(url
                .trim()
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .rsplit(':')
                .next()
                .unwrap_or("")
                .strip_suffix(".git")
                .unwrap_or_else(|| {
                    url.trim()
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .rsplit(':')
                        .next()
                        .unwrap_or("")
                })
                .to_owned())
        })
        .transpose()?;
    let mut fetched = BTreeMap::new();
    for b in &mut boxes {
        if name.as_deref().is_some_and(|n| field(b, "repo") != n) {
            continue;
        }
        let repo = only
            .clone()
            .unwrap_or_else(|| repos.join(field(b, "repo")).join("main"));
        if !fetched.contains_key(&repo) {
            let ready = success(git(
                &repo,
                &[
                    "fetch",
                    "-q",
                    "origin",
                    "+refs/heads/main:refs/remotes/origin/main",
                ],
            ))? || success(git(
                &repo,
                &["rev-parse", "-q", "--verify", "refs/remotes/origin/main"],
            ))?;
            fetched.insert(repo.clone(), ready);
        }
        if !fetched[&repo] {
            continue;
        }
        if field(b, "state") == "taken" && item_closed(&repo, field(b, "item"))? {
            if success(threads(&root, &["follow-started", &b["id"].to_string()]))? {
                println!(
                    "FOLLOWS box {} {}: its issue is closed, so the box is done",
                    b["id"],
                    field(b, "item")
                );
            }
            continue;
        }
        if field(b, "state") == "queued" {
            if !landed(&repo, field(b, "follows"))?
                || !success(threads(&root, &["follow-release", &b["id"].to_string()]))?
            {
                continue;
            }
            b["state"] = json!("taken");
            b["taken"] = json!(Timestamp::now().iso());
            println!(
                "FOLLOWS box {} {}: released, its predecessor {} is on origin/main",
                b["id"],
                field(b, "item"),
                field(b, "follows")
            );
        }
        let now = env::var("FOLLOWS_RELEASE_NOW")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f64>())
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| Timestamp::now().unix_seconds_f64());
        if let Some(denied) = gate(&root, b, &repo)?.filter(|s| !s.is_empty()) {
            alert(
                &root,
                b,
                &format!(
                    "denied by the spawn gate: {}",
                    denied
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(300)
                        .collect::<String>()
                ),
                now,
            )?;
        } else if start(b)? {
            let _ = threads(&root, &["follow-started", &b["id"].to_string()]);
            println!(
                "FOLLOWS box {} {}: started on {}",
                b["id"],
                field(b, "item"),
                field(b, "tier")
            );
        } else {
            let taken = Timestamp::parse_python_datetime(field(b, "taken"))
                .ok_or("invalid taken timestamp")?
                .unix_nanos() as f64
                / 1_000_000_000.;
            if now - taken >= 300. {
                alert(
                    &root,
                    b,
                    &format!(
                        "released at {} but not started within 5 minutes",
                        field(b, "taken")
                    ),
                    now,
                )?;
            }
        }
    }
    Ok(0)
}
const HELP: &str = r#"usage: follows-release [-h] [--repo-dir REPO_DIR]

follows-release: start each queued Follows box (threads queue follow) once its
predecessor is on origin/main. safe-push runs it with --repo-dir after a
landing on the default branch, reading each box's repository from FOLLOWS_RELEASE_ROOT/<repo>/main
(default ~/code). A Follows naming a SHA waits for that commit, one naming an
issue ref (optionally followed by a note) for that issue to be closed on GitHub, a leading SHA for that commit, and anything else for a commit with that exact subject. A released box
passes the spawn capacity gate and sends a staffing request through
`north fleet send` to the assigned parent session. A box
the gate denies, or one released but not started within 5 minutes
(FOLLOWS_RELEASE_NOW fakes the clock), is sent to that parent through Fleet;
a released box is retried on every later pass.

options:
  -h, --help           show this help message and exit
  --repo-dir REPO_DIR  the repository a landing just reached; only its boxes
                       are considered
"#;
pub(crate) fn run(args: &[String]) -> u8 {
    let mut only = None;
    let mut unknown = Vec::new();
    let negative_number = Regex::new(r"\A-\d+$|\A-\d*\.\d+$").unwrap();
    let mut i = 0;
    while i < args.len() {
        let (key, inline) = args[i]
            .split_once('=')
            .map_or((args[i].as_str(), None), |(k, v)| (k, Some(v)));
        if key.starts_with("-h") || (key.len() > 2 && "--help".starts_with(key)) {
            if let Some(value) = inline.filter(|_| key == "-h" || key.starts_with("--")) {
                return usage_error(&format!(
                    "argument -h/--help: ignored explicit argument '{value}'"
                ));
            }
            print!("{HELP}");
            return 0;
        }
        if key.starts_with("--") && key.len() > 2 && "--repo-dir".starts_with(key) {
            let path = if let Some(path) = inline {
                path
            } else {
                i += 1;
                if i >= args.len()
                    || (args[i] != "-"
                        && args[i].starts_with('-')
                        && !negative_number.is_match(&args[i]))
                {
                    return usage_error("argument --repo-dir: expected one argument");
                }
                &args[i]
            };
            only = if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            };
        } else if args[i] == "--" {
            unknown.extend(args[i..].iter().map(String::as_str));
            break;
        } else {
            unknown.push(&args[i]);
        }
        i += 1;
    }
    if !unknown.is_empty() {
        return usage_error(&format!("unrecognized arguments: {}", unknown.join(" ")));
    }
    match execute(only) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("follows-release: {error}");
            1
        }
    }
}
fn usage_error(message: &str) -> u8 {
    eprintln!(
        "{}\nfollows-release: error: {message}",
        HELP.lines().next().unwrap()
    );
    2
}

#[cfg(test)]
#[path = "follows_release_tests.rs"]
mod tests;
