//! `north land adopt` (north#1575): the fleet's side of the lander adopting a finished lane whose session ended. The
//! fleet's session records say which sessions are live, `safe-push --to main --wait` lands the lane, and the lane's
//! issue, from its commit's `Item:` trailer or the issue `north lane new` recorded, hears why it did not.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::platform::github;
use north_lander::land::adopt::{self, Adopter, Inputs, Sessions};

const TAIL: usize = 40;

struct Fleet(Vec<crate::fleet::records::RecordedSession>);

impl Sessions for Fleet {
    fn live(&self, session: &str, lane: &Path) -> bool {
        let lane = canonical(lane);
        let named: Vec<_> = self.0.iter().filter(|record| record.name == session).collect();
        named.is_empty() || named.iter().any(|record| !record.ended) || self.0.iter().any(|record| !record.ended && canonical(&record.cwd).starts_with(&lane))
    }
}

struct SafePush;

impl Adopter for SafePush {
    fn land(&self, lane: &Path) -> Result<String, String> {
        let north = std::env::current_exe().map_err(|error| error.to_string())?;
        let output = Command::new(north).args(["safe-push", "--to", "main", "--wait"]).current_dir(lane).env_remove("NORTH_FLEET_SESSION").stdin(Stdio::null()).output().map_err(|error| format!("safe-push did not start: {error}"))?;
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let lines: Vec<&str> = text.lines().collect();
        if output.status.success() {
            return Ok(lines.iter().rev().find(|line| line.contains("landed")).copied().unwrap_or("landed").trim().to_owned());
        }
        Err(format!("safe-push --to main --wait exited {}:\n{}", output.status.code().map_or_else(|| "on a signal".to_owned(), |code| code.to_string()), lines[lines.len().saturating_sub(TAIL)..].join("\n")))
    }

    fn file(&self, lane: &Path, why: &str) -> Result<String, String> {
        let item = git(lane, &["log", "-1", "--format=%(trailers:key=Item,valueonly)"]).and_then(|items| items.lines().next().map(str::to_owned)).filter(|item| !item.is_empty()).or_else(|| crate::lanes::lane_issue(lane)).ok_or("the lane names no issue in an Item: trailer or its lane record")?;
        let (repo, number) = item.trim().split_once('#').ok_or_else(|| format!("{item} is not REPO#N"))?;
        let number: u64 = number.parse().map_err(|_| format!("{item} is not REPO#N"))?;
        let repo = if repo.contains('/') { repo.to_owned() } else { format!("{}/{repo}", owner(lane).ok_or("the lane's origin names no owner")?) };
        github::comment(&repo, number, why)?;
        Ok(format!("{repo}#{number}"))
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            eprintln!("{}", adopt::USAGE);
            return 64;
        }
    };
    let inputs = Inputs::here();
    let sessions = Fleet(crate::fleet::records::recorded_sessions(&inputs.state.join("north/fleet")));
    adopt::adopt(&inputs, &sessions, &SafePush, dry_run).iter().for_each(|line| println!("{line}"));
    0
}

fn owner(lane: &Path) -> Option<String> {
    let url = git(lane, &["remote", "get-url", "origin"])?;
    let path = url.trim_end_matches(".git").trim_end_matches('/');
    let mut parts = path.rsplit(['/', ':']);
    parts.next()?;
    parts.next().map(str::to_owned)
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
