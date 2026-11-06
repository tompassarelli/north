use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::foundation::clock::Timestamp;

use crate::platform::follows::{Predecessor, predecessor, subject_on_main};
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
fn threads(args: &[&str]) -> std::io::Result<Output> {
    Command::new(std::env::current_exe()?)
        .args(["threads", "queue"])
        .args(args)
        .output()
}
fn claude_tier(tier: &str) -> bool {
    ["worker", "claude:", "sonnet", "opus", "haiku", "fable"]
        .iter()
        .any(|prefix| tier.starts_with(prefix))
}
fn planned_account(plan: &str) -> Option<String> {
    let target = plan
        .lines()
        .find_map(|line| line.split_once(": target ").map(|(_, rest)| rest).or_else(|| line.strip_prefix("target ")))?;
    let mut words = target.split_whitespace();
    let first = words.next()?;
    let account = words.next()?.strip_prefix("claude:")?;
    (!first.ends_with(':') && first != "anthropic-cloud").then(|| account.to_owned())
}
fn plan(b: &Value) -> Option<String> {
    if !claude_tier(field(b, "tier")) {
        return None;
    }
    let mut brief = tempfile::Builder::new().prefix("follows-brief").suffix(".md").tempfile().ok()?;
    brief.write_all(field(b, "brief").as_bytes()).ok()?;
    let out = Command::new("north")
        .args(["agents", "plan", "--place"])
        .arg(brief.path())
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    planned_account(&String::from_utf8_lossy(&out.stdout))
}
fn gate(b: &Value, repo: &Path, profile: Option<&Path>) -> Result<Option<String>, String> {
    let tier = field(b, "tier");
    let tool = if claude_tier(tier) {
        json!({"tool_name":"Agent","tool_input":{"subagent_type":tier,"description":b["item"],"prompt":b["brief"],"run_in_background":true}})
    } else {
        json!({"tool_name":"spawn_agent","tool_input":{"message":b["brief"]}})
    };
    let mut event = tool;
    event["hook_event_name"] = json!("PreToolUse");
    event["cwd"] = json!(repo);
    let mut command = Command::new("north");
    if let Some(profile) = profile {
        command.env("CLAUDE_CONFIG_DIR", profile);
    }
    let mut child = command
        .args(["hook", "spawn-capacity"])
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
fn start(b: &Value, account: Option<&str>) -> Result<bool, String> {
    let tier = field(b, "tier");
    let route = match account {
        Some(account) => format!(" Launch it on account {account} (`north fleet launch --account {account}`), the account `north agents plan` picked and the spawn gate admitted."),
        None if claude_tier(tier) => " Call Claude Agent with run_in_background: true.".to_owned(),
        None => String::new(),
    };
    let message = format!(
        "{}\nStaff this queued box now as a fresh worker at {tier}.{route} If you staff it under a worker of your own, run `north threads queue follow-started {}` so it is not sent again.",
        field(b, "brief"),
        b["id"]
    );
    let Ok(mut child) = Command::new("north")
        .args(["fleet", "send", "--from", "service:follows-release", field(b, "parent"), &message])
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
fn alert(b: &Value, why: &str, now: f64) -> Result<(), String> {
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
        &["follow-alert", &b["id"].to_string(), &line],
    ))? {
        return Ok(());
    }
    let _ = Command::new("north")
        .args(["fleet", "send", "--from", "service:follows-release", field(b, "parent"), &line])
        .stdin(Stdio::null())
        .status();
    println!("{line}");
    Ok(())
}
trait Desk {
    fn landed(&self, repo: &Path, follows: &str) -> Result<bool, String>;
    fn item_closed(&self, repo: &Path, item: &str) -> Result<bool, String>;
    fn step(&self, verb: &str, b: &Value) -> Result<bool, String>;
    fn plan(&self, b: &Value) -> Option<String>;
    fn gate(&self, b: &Value, repo: &Path, account: Option<&str>) -> Result<Option<String>, String>;
    fn start(&self, b: &Value, account: Option<&str>) -> Result<bool, String>;
    fn alert(&self, b: &Value, why: &str, now: f64) -> Result<(), String>;
}
struct Live {
    profiles: PathBuf,
}
impl Desk for Live {
    fn landed(&self, repo: &Path, follows: &str) -> Result<bool, String> {
        landed(repo, follows)
    }
    fn item_closed(&self, repo: &Path, item: &str) -> Result<bool, String> {
        item_closed(repo, item)
    }
    fn step(&self, verb: &str, b: &Value) -> Result<bool, String> {
        success(threads(&[verb, &b["id"].to_string()]))
    }
    fn plan(&self, b: &Value) -> Option<String> {
        plan(b)
    }
    fn gate(&self, b: &Value, repo: &Path, account: Option<&str>) -> Result<Option<String>, String> {
        let profile = account.map(|label| self.profiles.join(format!("claude-{label}")));
        gate(b, repo, profile.as_deref())
    }
    fn start(&self, b: &Value, account: Option<&str>) -> Result<bool, String> {
        start(b, account)
    }
    fn alert(&self, b: &Value, why: &str, now: f64) -> Result<(), String> {
        alert(b, why, now)
    }
}
fn tend(desk: &impl Desk, b: &mut Value, repo: &Path, now: f64) -> Result<(), String> {
    if field(b, "state") == "queued" {
        if !desk.landed(repo, field(b, "follows"))? || !desk.step("follow-release", b)? {
            return Ok(());
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
    if desk.item_closed(repo, field(b, "item"))? {
        if desk.step("follow-started", b)? {
            println!("FOLLOWS box {} {}: its issue is closed, so the box is done", b["id"], field(b, "item"));
        }
        return Ok(());
    }
    let account = desk.plan(b);
    if let Some(denied) = desk.gate(b, repo, account.as_deref())?.filter(|s| !s.is_empty()) {
        let first = denied.lines().next().unwrap_or("").chars().take(300).collect::<String>();
        desk.alert(b, &format!("denied by the spawn gate: {first}"), now)
    } else if desk.start(b, account.as_deref())? {
        let _ = desk.step("follow-started", b);
        println!("FOLLOWS box {} {}: started on {}", b["id"], field(b, "item"), field(b, "tier"));
        Ok(())
    } else {
        let taken = Timestamp::parse_python_datetime(field(b, "taken"))
            .ok_or("invalid taken timestamp")?
            .unix_nanos() as f64
            / 1_000_000_000.;
        if now - taken >= 300. {
            desk.alert(b, &format!("released at {} but not started within 5 minutes", field(b, "taken")), now)?;
        }
        Ok(())
    }
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
    let listing = threads(&["follow-list"]).map_err(|e| e.to_string())?;
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
    let now = env::var("FOLLOWS_RELEASE_NOW")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>())
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| Timestamp::now().unix_seconds_f64());
    let desk = Live { profiles: state.join("north/profiles") };
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
        tend(&desk, b, &repo, now)?;
    }
    Ok(0)
}
const HELP: &str = r#"usage: follows-release [-h] [--repo-dir REPO_DIR]

follows-release: start each queued Follows box (threads queue follow) once its
predecessor is on origin/main. safe-push runs it with --repo-dir after a
landing on the default branch, reading each box's repository from FOLLOWS_RELEASE_ROOT/<repo>/main
(default ~/code). A Follows naming a SHA waits for that commit, one naming an
issue ref (optionally followed by a note) for that issue to be closed on GitHub, a leading SHA for that commit, and anything else for a commit with that exact subject. A released box
whose own item issue is closed is done. Any other released box passes the spawn
capacity gate, judged against the account `north agents plan --place` picks for
its brief, and sends a staffing request naming that account through
`north fleet send` to the assigned parent session. A box
the gate denies, or one released but not started within 5 minutes
(FOLLOWS_RELEASE_NOW fakes the clock), is sent to that parent through Fleet;
a released box is retried on every later pass.

options:
  -h, --help           show this help message and exit
  --repo-dir REPO_DIR  the repository a landing just reached; only its boxes
                       are considered
"#;
pub fn run(args: &[String]) -> u8 {
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
