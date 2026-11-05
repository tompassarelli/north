//! The guard on `north fleet forget`: a session that still owns a lane in the land queue is kept,
//! because the lander tells that session what became of the lane, and a forgotten one cannot hear it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{NorthError, NorthResult};

use super::registry::Controlled;

/// A ticket waiting in a land queue whose verdict goes to the session being forgotten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedLane {
    pub ticket: PathBuf,
    pub branch: String,
}

/// What a forget did: where it applied, and the queued lanes `--force` overrode.
pub struct Forgotten {
    pub controlled: Controlled,
    pub overrode: Vec<QueuedLane>,
}

#[derive(Deserialize)]
struct Ticket {
    root: PathBuf,
    branch: String,
    #[serde(default)]
    session: Option<String>,
}

/// The tickets waiting in any queue under `queues` that name the session `name` or whose lane is
/// its worktree `cwd`. A queue's waiting tickets lie directly in `queues/<key>/`; decided,
/// withdrawn and quarantined ones lie in its subdirectories and no longer report to anyone.
pub fn queued_lanes(queues: &Path, name: &str, cwd: Option<&Path>) -> Vec<QueuedLane> {
    let mut lanes: Vec<QueuedLane> = fs::read_dir(queues)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .flat_map(|queue| fs::read_dir(queue.path()).into_iter().flatten().filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json") && path.is_file())
        .filter_map(|path| {
            let ticket: Ticket = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
            let owned = ticket.session.as_deref() == Some(name) || cwd.is_some_and(|cwd| ticket.root == cwd);
            owned.then_some(QueuedLane { ticket: path, branch: ticket.branch })
        })
        .collect();
    lanes.sort_by(|left, right| left.ticket.cmp(&right.ticket));
    lanes
}

/// Lets a forget through when no lane is queued for `name`, or when `force` gives a reason to
/// drop the session anyway; otherwise refuses, naming each blocking ticket and its branch.
pub fn admit(name: &str, lanes: &[QueuedLane], force: Option<&str>) -> NorthResult<()> {
    match force.map(str::trim) {
        Some("") => Err(NorthError::Usage("fleet forget --force needs a REASON".into())),
        Some(_) => Ok(()),
        None if lanes.is_empty() => Ok(()),
        None => {
            let named: Vec<String> = lanes.iter().map(|lane| format!("{} (branch {})", lane.ticket.display(), lane.branch)).collect();
            Err(NorthError::Usage(format!(
                "fleet forget {name}: queued land tickets still report to it: {}; forget it once they are decided, or pass --force REASON",
                named.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::registry::Controlled;
    use super::super::registry::fixtures::{fleet, live};
    use super::*;

    fn ticket(home: &Path, area: &str, file: &str, session: &str, root: &str) -> PathBuf {
        let directory = home.join(".local/state/north/land-queue/560e3f31dbaa2415").join(area);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(file);
        let text = serde_json::json!({"origin": "https://github.com/tompassarelli/north", "root": root, "host": "nexus", "branch": "w33-concurrent", "head": "abc", "session": session, "queued_at": "t", "red_fix": null});
        fs::write(&path, text.to_string()).unwrap();
        path
    }

    #[test]
    fn a_pending_ticket_naming_the_session_refuses_the_forget_with_its_path_and_branch_and_keeps_the_session() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let _worker = live(&fleet, "lander-w33-failfast");
        let path = ticket(home.path(), "", "1-w33-concurrent.json", "lander-w33-failfast", "/code/north/worktrees/w33-concurrent");

        let Err(NorthError::Usage(why)) = fleet.forget_unless_queued("lander-w33-failfast", None) else { panic!("a queued lane refuses the forget") };

        assert!(why.contains(&format!("{} (branch w33-concurrent)", path.display())), "{why}");
        assert!(why.contains("--force REASON"), "{why}");
        assert!(fleet.session("lander-w33-failfast").is_ok(), "a refused forget keeps the session");
    }

    #[test]
    fn a_pending_ticket_whose_lane_is_the_sessions_worktree_refuses_the_forget_though_it_names_no_session() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let _worker = live(&fleet, "worker");
        ticket(home.path(), "", "1-w33-concurrent.json", "someone-else", "/tmp");

        assert!(matches!(fleet.forget_unless_queued("worker", None), Err(NorthError::Usage(why)) if why.contains("1-w33-concurrent.json")));
    }

    #[test]
    fn force_with_a_reason_forgets_a_session_whose_lane_is_queued_and_reports_the_lane_it_overrode() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let _worker = live(&fleet, "worker");
        let path = ticket(home.path(), "", "1-w33-concurrent.json", "worker", "/code/north/worktrees/w33-concurrent");

        assert!(matches!(fleet.forget_unless_queued("worker", Some(" ")), Err(NorthError::Usage(why)) if why.contains("needs a REASON")));
        let forgotten = fleet.forget_unless_queued("worker", Some("lane withdrawn by hand")).unwrap();

        assert!(matches!(forgotten.controlled, Controlled::Here));
        assert_eq!(forgotten.overrode, [QueuedLane { ticket: path, branch: "w33-concurrent".into() }]);
        assert!(fleet.session("worker").is_err());
    }

    #[test]
    fn a_session_whose_tickets_are_all_decided_withdrawn_or_quarantined_is_forgotten() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let _worker = live(&fleet, "worker");
        for area in ["done", "withdrawn", "quarantine"] {
            ticket(home.path(), area, "1-w33-concurrent.json", "worker", "/tmp");
        }
        ticket(home.path(), "", "2-other.json", "another-worker", "/code/north/worktrees/other");

        let forgotten = fleet.forget_unless_queued("worker", None).unwrap();

        assert!(forgotten.overrode.is_empty());
        assert!(fleet.session("worker").is_err());
    }

    #[test]
    fn a_host_with_no_land_queue_forgets_freely() {
        let home = tempfile::tempdir().unwrap();
        assert!(queued_lanes(&home.path().join("land-queue"), "worker", None).is_empty());
        assert!(admit("worker", &[], None).is_ok());
    }
}
