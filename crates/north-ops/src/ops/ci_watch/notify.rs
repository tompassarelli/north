use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use super::{Call, TITLE, gh, gh_json};
use crate::foundation::clock::Timestamp;

const REPOS: &str = "tompassarelli/wisp tompassarelli/north tompassarelli/firn tompassarelli/makgora";
const REVERT_AFTER_S: i64 = 15 * 60;

#[derive(Clone, Debug, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Session {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub trait Io {
    fn issues(&self, repo: &str) -> Result<Vec<Issue>, String>;
    fn run_finished(&self, repo: &str, run: &str) -> Option<Timestamp>;
    fn sessions(&self) -> Vec<Session>;
    fn send(&self, target: &str, text: &str) -> Result<(), String>;
}

static FIRST_COMMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^- First failing commit: ([0-9a-f]{40})").expect("commit pattern"));
static FIRST_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^- First failing commit: .*actions/runs/([0-9]*)").expect("run pattern"));
static LANE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^- Culprit lane: claude/land-(.*)-[0-9a-f]{12} ").expect("lane pattern"));

fn clean(text: &str) -> String {
    text.chars().take(300).filter(|c| !matches!(*c, '\0'..='\x1f' | '\x7f')).collect()
}

fn failing_tests(body: &str) -> String {
    let Some(start) = body.lines().position(|line| line.starts_with("## Failing tests")) else { return String::new() };
    let tests: Vec<&str> = body.lines().skip(start).filter_map(|line| line.strip_prefix("- ")).take(3).collect();
    clean(&tests.join(";"))
}

fn lanes(body: &str) -> BTreeSet<String> {
    body.lines().filter_map(|line| LANE.captures(line)).map(|c| clean(&c[1])).filter(|lane| !lane.is_empty()).collect()
}

fn live_session<'a>(sessions: &'a [Session], lane: &str) -> Option<&'a str> {
    let suffix = format!("/worktrees/{lane}");
    sessions.iter().find(|s| s.status != "exited" && s.cwd.as_deref().is_some_and(|cwd| cwd.ends_with(&suffix))).map(|s| s.name.as_str())
}

fn notify_issue(io: &dyn Io, repo: &str, issue: &Issue, sha: &str) -> bool {
    let short = &sha[..10];
    let tests = failing_tests(&issue.body);
    let tests = if tests.is_empty() { "no test named" } else { &tests };
    let lanes = lanes(&issue.body);
    if lanes.is_empty() {
        let text = format!("main is red in {repo} from {short} with no lane recorded for it ({tests}); {}.", issue.url);
        return io.send("proxy", &text).is_ok();
    }
    let run = FIRST_RUN.captures(&issue.body).map(|c| c[1].to_owned()).unwrap_or_default();
    let deadline = io
        .run_finished(repo, &run)
        .map(|at| format!("; a lone culprit is reverted at {}", Timestamp::from_unix_seconds(at.unix_seconds() + REVERT_AFTER_S).local_twelve_hour()))
        .unwrap_or_default();
    let sessions = io.sessions();
    let mut sent = true;
    for lane in &lanes {
        let text = format!(
            "main is red in {repo} from {short}: your lane {lane} landed in the range that broke it ({tests}); {}. Fix forward now{deadline}.",
            issue.url
        );
        let target = live_session(&sessions, lane);
        if let Some(target) = target {
            let _ = io.send(target, &text);
        }
        let note = target.map_or(format!("(no live session for lane {lane})"), |target| format!("(sent to {target})"));
        sent &= io.send("proxy", &format!("{note} {text}")).is_ok();
    }
    sent
}

pub fn notify(io: &dyn Io, repos: &[&str], state: &Path) -> Result<(), String> {
    std::fs::create_dir_all(state).map_err(|error| format!("{}: {error}", state.display()))?;
    for repo in repos {
        let issues = match io.issues(repo) {
            Ok(issues) => issues,
            Err(error) => {
                eprintln!("red-main-notify: {repo}: {error}");
                continue;
            }
        };
        for issue in issues.iter().filter(|i| i.title == TITLE) {
            let Some(sha) = FIRST_COMMIT.captures(&issue.body).map(|c| c[1].to_owned()) else { continue };
            let key = state.join(format!("{}-{}-{sha}", repo.replace('/', "_"), issue.number));
            if key.exists() {
                continue;
            }
            if notify_issue(io, repo, issue, &sha) {
                std::fs::write(&key, "").map_err(|error| format!("{}: {error}", key.display()))?;
            }
        }
    }
    Ok(())
}

pub fn state_dir() -> PathBuf {
    if let Some(state) = std::env::var_os("RED_MAIN_STATE") {
        return state.into();
    }
    let home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    home.join("north/red-main-notify")
}

pub struct Shell;

impl Io for Shell {
    fn issues(&self, repo: &str) -> Result<Vec<Issue>, String> {
        let search = format!("\"{TITLE}\" in:title");
        gh_json(&["issue", "list", "-R", repo, "--state", "open", "--search", &search, "--json", "number,title,url,body"])
    }

    fn run_finished(&self, repo: &str, run: &str) -> Option<Timestamp> {
        let at = gh(&["run", "view", run, "-R", repo, "--json", "updatedAt", "--jq", ".updatedAt"]).ok()?;
        Timestamp::parse_rfc3339(at.trim())
    }

    fn sessions(&self) -> Vec<Session> {
        north(&["fleet", "list", "--json"]).ok().and_then(|out| serde_json::from_str(&out).ok()).unwrap_or_default()
    }

    fn send(&self, target: &str, text: &str) -> Result<(), String> {
        north(&["fleet", "send", "--from", "service:red-main-notify", target, text]).map(drop)
    }
}

fn north(args: &[&str]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let exe = exe.to_string_lossy();
    Call::new(&exe, args).output()
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.first().is_some_and(|a| a == "-h" || a == "--help") {
        println!("usage: north red-main-notify");
        println!("Sends each open \"main is red\" issue's culprit lane's session and the proxy one north fleet send, once per culprit commit.");
        println!("Reads the issue's \"Culprit lane:\" and \"First failing commit:\" lines (wisp:host/checks/mainRed.ts); the revert deadline is 15 minutes after the red run.");
        return 0;
    }
    if !arguments.is_empty() {
        eprintln!("usage: north red-main-notify");
        return 2;
    }
    let repos = std::env::var("RED_MAIN_REPOS").unwrap_or_else(|_| REPOS.into());
    match notify(&Shell, &repos.split_whitespace().collect::<Vec<_>>(), &state_dir()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("red-main-notify: {error}");
            1
        }
    }
}

