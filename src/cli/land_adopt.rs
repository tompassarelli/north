//! `north land adopt` (north#1575): the fleet's side of the lander adopting a finished lane whose session ended. The
//! fleet's session records say which sessions are live, `safe-push --to main --wait` lands the lane from inside a
//! worker's domain built for it, and the lane's issue, from its commit's `Item:` trailer or the issue `north lane new`
//! recorded for its branch in the main repository, hears why it did not, only when that issue is in the main repository's own origin and through gh run in a worker's domain, where gh answers as the agent App or not at all.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::Parser;
use north_base::platform::gh_shim;
use north_lander::land::adopt::{self, Adopter, Inputs, Lane, Refused, Sessions};

const TAIL: usize = 40;

struct Fleet(Vec<crate::fleet::records::RecordedSession>);

impl Sessions for Fleet {
    fn live(&self, session: &str, lane: &Path) -> bool {
        let lane = canonical(lane);
        let named: Vec<_> = self.0.iter().filter(|record| record.name == session).collect();
        named.is_empty() || named.iter().any(|record| !record.ended) || self.0.iter().any(|record| !record.ended && canonical(&record.cwd).starts_with(&lane))
    }
}

struct SafePush<'a> {
    jail: &'a [OsString],
    north: PathBuf,
}

impl Adopter for SafePush<'_> {
    fn land(&self, lane: &Lane) -> Result<String, Refused> {
        let output = lane.command(self.jail, &self.north).args(["safe-push", "--to", "main", "--wait"]).env_remove("NORTH_FLEET_SESSION").output().map_err(|error| Refused { summary: "safe-push did not start".to_owned(), detail: error.to_string() })?;
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let lines: Vec<&str> = text.lines().collect();
        if output.status.success() {
            return Ok(lines.iter().rev().find(|line| line.contains("landed")).copied().unwrap_or("landed").trim().to_owned());
        }
        Err(Refused { summary: format!("safe-push --to main --wait exited {}", output.status.code().map_or_else(|| "on a signal".to_owned(), |code| code.to_string())), detail: lines[lines.len().saturating_sub(TAIL)..].join("\n") })
    }

    fn file(&self, lane: &Lane, why: &str) -> Result<String, String> {
        let git = |arguments: &[&str]| lane.git(self.jail, arguments);
        let recorded = || crate::lanes::lane_issue(lane.common.parent()?, &git(&["branch", "--show-current"])?);
        let item = git(&["log", "-1", "--format=%(trailers:key=Item,valueonly)"]).and_then(|items| items.lines().next().map(str::to_owned)).filter(|item| !item.is_empty()).or_else(recorded).ok_or("the lane names no issue in an Item: trailer or its lane record")?;
        let (repo, number) = item.trim().split_once('#').ok_or_else(|| format!("{item} is not REPO#N"))?;
        let number: u64 = number.parse().map_err(|_| format!("{item} is not REPO#N"))?;
        let origin = lane.main().git(self.jail, &["remote", "get-url", "origin"]).as_deref().and_then(origin_repository).ok_or("the main repository's origin names no GitHub repository")?;
        let named = if repo.contains('/') { repo.to_owned() } else { format!("{}/{repo}", origin.split_once('/').map_or("", |(owner, _)| owner)) };
        if !named.eq_ignore_ascii_case(&origin) {
            return Err(format!("{item} names an issue outside {origin}, the lane's own repository"));
        }
        let gh = std::env::var("PATH").ok().as_deref().and_then(gh_shim::first_on_path).filter(|gh| gh_shim::is_shim(gh)).ok_or("the first gh on PATH is no north gh shim, so it would answer as a personal login")?;
        let posted = lane.main().command(self.jail, &gh).args(["issue", "comment", &number.to_string(), "-R", &origin, "--body", why]).env_remove("GH_TOKEN").env_remove("GITHUB_TOKEN").status().map_err(|error| format!("gh did not start: {error}"))?;
        if !posted.success() {
            return Err(format!("gh issue comment {number} -R {origin} in a worker's domain failed"));
        }
        Ok(format!("{origin}#{number}"))
    }
}

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true)]
struct Options {
    #[arg(long)]
    dry_run: bool,
}

pub fn run(arguments: &[String]) -> u8 {
    let Ok(Options { dry_run }) = north_core::arguments::parse(arguments) else {
        eprintln!("{}", adopt::USAGE);
        return 64;
    };
    let inputs = Inputs::here();
    let sessions = Fleet(crate::fleet::records::recorded_sessions(&inputs.state.join("north/fleet")));
    let adopter = SafePush { jail: &inputs.jail, north: north_base::platform::north_binary::own_binary() };
    adopt::adopt(&inputs, &sessions, &adopter, dry_run).iter().for_each(|line| println!("{line}"));
    0
}

fn origin_repository(url: &str) -> Option<String> {
    let path = url.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.rsplit(['/', ':']);
    let name = parts.next().filter(|name| !name.is_empty())?;
    let owner = parts.next().filter(|owner| !owner.is_empty())?;
    Some(format!("{owner}/{name}"))
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
