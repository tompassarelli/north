//! A committed lane whose session ended without handing it in: the run ledger's `lane` column names the lane each
//! fleet session made, and the fleet tick hands such a lane to `north finish` as that session, with the check its
//! brief's `Done when:` named, once per lane tip (north#1036).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rusqlite::Connection;

use north_base::platform::box_oracles::done_when_command;

const NEWEST: &str = "SELECT lane, fleet_name, brief_text FROM runs r WHERE lane IS NOT NULL AND fleet_name IS NOT NULL AND role = 'worker'
     AND NOT EXISTS (SELECT 1 FROM runs later WHERE later.lane = r.lane AND later.started > r.started)";
const HANDED: &str = "north-committed-lane-handed";
const LOG: &str = "north-committed-lane.log";

#[derive(Debug, PartialEq, Eq)]
pub struct Abandoned {
    pub lane: PathBuf,
    pub session: String,
    pub check: String,
    pub head: String,
}

/// The lanes the newest run of each names whose session `ended` says has ended, that hold commits origin/main lacks,
/// that no pending ticket `queued` names and that the tick has not handed at their tip before.
pub fn found(db: &Connection, ended: impl Fn(&str) -> bool, queued: impl Fn(&Path) -> bool) -> Vec<Abandoned> {
    let Ok(mut statement) = db.prepare(NEWEST) else { return Vec::new() };
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)));
    let rows: Vec<_> = rows.into_iter().flatten().flatten().collect();
    rows.into_iter()
        .filter_map(|(lane, session, brief)| {
            let lane = PathBuf::from(lane);
            let check = done_when_command(brief.as_deref()?)?.to_owned();
            if !lane.is_dir() || !ended(&session) {
                return None;
            }
            let head = git(&lane, &["rev-parse", "HEAD"])?;
            let branch = git(&lane, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
            let ahead: u32 = git(&lane, &["rev-list", "--count", "refs/remotes/origin/main..HEAD"])?.parse().ok()?;
            let handed = git_path(&lane, HANDED).and_then(|path| fs::read_to_string(path).ok());
            if branch == "main" || ahead == 0 || handed.as_deref().map(str::trim) == Some(head.as_str()) || queued(&lane) {
                return None;
            }
            Some(Abandoned { lane, session, check, head })
        })
        .collect()
}

/// Starts `north finish` in the lane as its session, naming every path the lane's commits change, and marks the tip
/// handed so a failed check is not rerun every tick; its output goes to the lane's git directory.
pub fn hand(abandoned: &Abandoned, north: &Path) -> Result<(), String> {
    let lane = &abandoned.lane;
    let marker = git_path(lane, HANDED).ok_or("the lane has no git directory")?;
    let log = git_path(lane, LOG).ok_or("the lane has no git directory")?;
    let subject = git(lane, &["log", "-1", "--format=%s"]).ok_or("the lane's head has no subject")?;
    let paths = git(lane, &["diff", "--name-only", "refs/remotes/origin/main...HEAD"]).ok_or("cannot list the lane's changes")?;
    fs::write(&marker, format!("{}\n", abandoned.head)).map_err(|error| format!("{}: {error}", marker.display()))?;
    let output = fs::File::create(&log).map_err(|error| format!("{}: {error}", log.display()))?;
    let errors = output.try_clone().map_err(|error| format!("{}: {error}", log.display()))?;
    Command::new(north)
        .args(["finish", "--check", &abandoned.check, "--message", &subject, "--"])
        .args(paths.lines().filter(|path| !path.is_empty()).map(|path| lane.join(path)))
        .current_dir(lane)
        .env("NORTH_FLEET_SESSION", &abandoned.session)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .spawn()
        .map(drop)
        .map_err(|error| format!("north finish: {error}"))
}

fn git_path(lane: &Path, name: &str) -> Option<PathBuf> {
    git(lane, &["rev-parse", "--path-format=absolute", "--git-path", name]).map(PathBuf::from)
}

fn git(lane: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(lane).args(arguments).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
#[path = "committed_lane_tests.rs"]
mod tests;
