use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::parse::{self, Push};
use super::{BetIssue, CloudSession, Facts, FarmRun, Landing, LandingLock, LimitStop, LockHolder, Place, ReleaseIssue, SessionFact, WEEK, Waiter};
use crate::platform::process;

const OWNER: &str = "tompassarelli";
const BET_REPOS: [&str; 3] = ["north", "smashcraft", "wisp"];
const FARM_REPOS: [&str; 2] = ["smashcraft", "wisp"];
const FARM_TIMINGS_PER_REFRESH: usize = 60;
const REMOTE_LIMIT: Duration = Duration::from_secs(60);
const CLOUD_MARK: &str = "--- north ops cloud ---";

pub(crate) struct Sources {
    pub state: PathBuf,
    pub code: PathBuf,
    pub ops: PathBuf,
    pub remote: bool,
}

impl Sources {
    pub(crate) fn from_home(home: &Path, remote: bool) -> Self {
        let state = home.join(".local/state");
        Self { ops: state.join("north/ops"), state, code: home.join("code"), remote }
    }
}

pub(crate) fn gather(sources: &Sources, sessions: Vec<SessionFact>, now: i64) -> Facts {
    let since = now - WEEK;
    let mut facts = Facts { now, sessions, ..Facts::default() };
    let agents = sources.state.join("agents");
    facts.pace = read(&agents.join("pace-control.json")).and_then(|text| serde_json::from_str(&text).ok());
    facts.runs = parse::capacity_runs(&read(&agents.join("machine-capacity-usage.jsonl")).unwrap_or_default(), Place::Laptop, since);
    facts.builds = parse::builds(&read(&agents.join("handoffs/tom-build.log")).unwrap_or_default(), since);
    facts.load = read(Path::new("/proc/loadavg")).and_then(|text| parse::load(&text, cores()));
    facts.locks = landing_locks(&sources.state.join("safe-push"));
    facts.landings = landings(sources, since);
    facts.limit_stops = facts.sessions.iter().filter(|session| session.live).filter_map(limit_stop).collect();
    facts.checks = parse::check_runs(&read(&sources.ops.join("checks.tsv")).unwrap_or_default(), since);
    if sources.remote {
        remote(sources, &mut facts, since);
    }
    facts
}

fn remote(sources: &Sources, facts: &mut Facts, since: i64) {
    let (nexus, release, bets, farm) = std::thread::scope(|scope| {
        let nexus = scope.spawn(nexus_listing);
        let release = scope.spawn(release_issues);
        let bets = scope.spawn(bet_issues);
        let farm = scope.spawn(|| farm_runs(&sources.ops, since));
        (nexus.join().ok().flatten(), release.join().ok().flatten(), bets.join().ok().flatten(), farm.join().ok().flatten())
    });
    match nexus {
        Some((log, cloud)) => {
            facts.runs.extend(parse::capacity_runs(&log, Place::Nexus, since));
            facts.nexus = true;
            facts.cloud = cloud;
        }
        None => facts.gaps.push("nexus unreachable: no nexus CPU or cloud sessions".into()),
    }
    if facts.cloud.is_none() {
        facts.gaps.push("cloud session list unavailable".into());
    }
    facts.release = release.or_else(|| gap(&mut facts.gaps, "GitHub release milestone unavailable"));
    facts.bet_issues = bets.or_else(|| gap(&mut facts.gaps, "GitHub bet issues unavailable"));
    match farm {
        Some((runs, untimed)) => {
            if untimed > 0 {
                facts.gaps.push(format!("{untimed} hosted farm runs not timed yet"));
            }
            facts.farm = Some(runs);
        }
        None => facts.gaps.push("hosted farm runs unavailable".into()),
    }
}

fn gap<T>(gaps: &mut Vec<String>, message: &str) -> Option<T> {
    gaps.push(message.to_owned());
    None
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn cores() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = process::run_within(program, args, REMOTE_LIMIT)?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn gh_json(args: &[&str]) -> Option<Value> {
    serde_json::from_str(&output("gh", args)?).ok()
}

fn nexus_listing() -> Option<(String, Option<Vec<CloudSession>>)> {
    let script = format!(
        "cat ~/.local/state/agents/machine-capacity-usage.jsonl; echo '{CLOUD_MARK}'; bash ~/code/north/main/harness/skills/workers/scripts/cloud-sessions.sh list && echo '{CLOUD_MARK}'"
    );
    let text = output("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "nexus", &script])
        .or_else(|| output("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "nexus", "cat ~/.local/state/agents/machine-capacity-usage.jsonl"]))?;
    let mark = format!("{CLOUD_MARK}\n");
    let mut parts = text.split(mark.as_str());
    let log = parts.next().unwrap_or_default().to_owned();
    let cloud = parts.next().filter(|_| parts.next().is_some()).map(parse::cloud_sessions);
    Some((log, cloud))
}

fn release_issues() -> Option<Vec<ReleaseIssue>> {
    let repo = format!("{OWNER}/smashcraft");
    let listing = gh_json(&["issue", "list", "-R", &repo, "--milestone", "Public 0.1", "--state", "all", "--limit", "300", "--json", "number,state,closedAt,labels"])?;
    Some(parse::release_issues(&listing))
}

fn bet_issues() -> Option<Vec<BetIssue>> {
    let listings: Vec<(&str, Value)> = BET_REPOS
        .into_iter()
        .filter_map(|repo| {
            let full = format!("{OWNER}/{repo}");
            Some((repo, gh_json(&["issue", "list", "-R", &full, "--label", "bet", "--state", "all", "--limit", "100", "--json", "number,title,state,body,closedAt,labels"])?))
        })
        .collect();
    (!listings.is_empty()).then(|| listings.iter().flat_map(|(repo, listing)| parse::bet_issues(repo, listing)).collect())
}

fn farm_runs(ops: &Path, since: i64) -> Option<(Vec<FarmRun>, usize)> {
    let cache_path = ops.join("farm-jobs.json");
    let mut cache: BTreeMap<String, f64> = read(&cache_path).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let mut runs = Vec::new();
    let (mut untimed, mut fetched) = (0, 0);
    for repo in FARM_REPOS {
        let full = format!("{OWNER}/{repo}");
        let Some(listing) = gh_json(&["run", "list", "-R", &full, "--workflow", "farm-test.yml", "--limit", "500", "--json", "databaseId,createdAt,status"]) else {
            continue;
        };
        for run in listing.as_array().into_iter().flatten() {
            let (Some(id), Some(created_at)) = (run["databaseId"].as_u64(), run["createdAt"].as_str().and_then(parse::seconds)) else { continue };
            if created_at < since || run["status"] != "completed" {
                continue;
            }
            let key = format!("{repo}/{id}");
            if !cache.contains_key(&key) && fetched < FARM_TIMINGS_PER_REFRESH {
                fetched += 1;
                if let Some(seconds) = job_seconds(&full, id) {
                    cache.insert(key.clone(), seconds);
                }
            }
            match cache.get(&key) {
                Some(seconds) => runs.push(FarmRun { repo: repo.to_owned(), id, created_at, job_seconds: *seconds }),
                None => untimed += 1,
            }
        }
    }
    cache.retain(|key, _| runs.iter().any(|run| format!("{}/{}", run.repo, run.id) == *key));
    let _ = fs::create_dir_all(ops).and_then(|()| fs::write(&cache_path, serde_json::to_string(&cache).unwrap_or_default()));
    Some((runs, untimed))
}

fn job_seconds(repo: &str, id: u64) -> Option<f64> {
    let path = format!("repos/{repo}/actions/runs/{id}/jobs?per_page=100");
    let jobs = gh_json(&["api", &path])?;
    Some(
        jobs["jobs"]
            .as_array()?
            .iter()
            .filter_map(|job| Some(parse::seconds(job["completed_at"].as_str()?)? - parse::seconds(job["started_at"].as_str()?)?))
            .map(|seconds| seconds.max(0) as f64)
            .sum(),
    )
}

fn landing_locks(dir: &Path) -> Vec<LandingLock> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut locks: Vec<LandingLock> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".lock").map(str::to_owned))
        .filter_map(|key| landing_lock(dir, &key))
        .collect();
    locks.sort_by(|a, b| a.repo.cmp(&b.repo));
    locks
}

fn landing_lock(dir: &Path, key: &str) -> Option<LandingLock> {
    let lock = dir.join(format!("{key}.lock"));
    let (pid, holder_dir) = parse::lock_holder(&read(&lock).unwrap_or_default()).unzip();
    let held_since = fs::metadata(&lock).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let tickets: Vec<parse::Ticket> = fs::read_dir(dir.join(format!("{key}.queue")))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            if name.ends_with(".result") {
                return None;
            }
            parse::ticket(&name, &read(&entry.path()).unwrap_or_default())
        })
        .filter(|ticket| live_safe_push(ticket.pid))
        .collect();
    let holder = pid.filter(|pid| live_safe_push(*pid)).map(|pid| LockHolder {
        pid,
        dir: holder_dir.clone().unwrap_or_default(),
        session: tickets.iter().find(|ticket| ticket.pid == pid).and_then(|ticket| ticket.session.clone()),
    });
    let repo = holder_dir
        .as_deref()
        .and_then(parse::repo_of)
        .or_else(|| tickets.iter().find_map(|ticket| parse::repo_of(&ticket.dir)))
        .unwrap_or_else(|| key.to_owned());
    let waiters = tickets
        .into_iter()
        .filter(|ticket| Some(ticket.pid) != holder.as_ref().map(|holder| holder.pid))
        .map(|ticket| Waiter { session: ticket.session, branch: ticket.branch, since: ticket.since })
        .collect();
    Some(LandingLock { repo, holder, held_since, waiters })
}

fn live_safe_push(pid: u32) -> bool {
    fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|cmdline| String::from_utf8_lossy(&cmdline).contains("safe-push"))
}

fn landings(sources: &Sources, since: i64) -> Vec<Landing> {
    let pushes = parse::pushes(&read(&sources.state.join("safe-push/pushes.tsv")).unwrap_or_default(), since);
    let repos: BTreeSet<&str> = pushes.iter().map(|push| push.repo.as_str()).collect();
    let commits: BTreeMap<&str, BTreeMap<String, (i64, String)>> =
        repos.into_iter().map(|repo| (repo, commits(&sources.code.join(repo).join("main"), &pushes, repo))).collect();
    pushes
        .iter()
        .map(|push| {
            let commit = commits.get(push.repo.as_str()).and_then(|known| known.get(&push.sha));
            Landing {
                at: push.at,
                repo: push.repo.clone(),
                sha: push.sha.clone(),
                committed_at: commit.map(|(at, _)| *at),
                issue_refs: commit.map(|(_, message)| parse::issue_refs(message)).unwrap_or_default(),
            }
        })
        .collect()
}

fn commits(checkout: &Path, pushes: &[Push], repo: &str) -> BTreeMap<String, (i64, String)> {
    let shas: Vec<&str> = pushes.iter().filter(|push| push.repo == repo).map(|push| push.sha.as_str()).collect();
    let input = shas.join("\n") + "\n";
    let Some(batch) = process::run_with_input("git", checkout, &["cat-file", "--batch"], input.as_bytes()) else {
        return BTreeMap::new();
    };
    parse::commit_batch(&batch)
}

const STREAM_TAIL_BYTES: u64 = 256 * 1024;

fn limit_stop(session: &SessionFact) -> Option<LimitStop> {
    let generation = fs::read_dir(&session.host_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| Some((entry.file_name().to_str()?.strip_prefix('g')?.parse::<u32>().ok()?, entry.path())))
        .max_by_key(|(number, _)| *number)?
        .1;
    parse::limit_stop(&tail(&generation.join("stream.jsonl"))?, &session.name, &session.account)
}

fn tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(STREAM_TAIL_BYTES))).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Some(match length > STREAM_TAIL_BYTES {
        true => text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_owned()),
        false => text,
    })
}
