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

/// Points the waiting ticket at `ticket` to `session`, so the lander's verdict reaches the lane's new owner.
pub fn reassign(ticket: &Path, session: &str) -> NorthResult<()> {
    let mut text: serde_json::Value = serde_json::from_str(&fs::read_to_string(ticket)?)?;
    text["session"] = serde_json::Value::String(session.to_owned());
    let staged = ticket.with_extension("json.handoff");
    fs::write(&staged, serde_json::to_vec_pretty(&text)?)?;
    fs::rename(&staged, ticket)?;
    Ok(())
}

/// Lets a forget through when no lane is queued for `name` or an `heir` takes them over, or when `force`
/// gives a reason to drop the session anyway; otherwise refuses, naming each blocking ticket and its branch.
pub fn admit(name: &str, lanes: &[QueuedLane], heir: Option<&str>, force: Option<&str>) -> NorthResult<()> {
    match force.map(str::trim) {
        Some("") => Err(NorthError::Usage("fleet forget --force needs a REASON".into())),
        Some(_) => Ok(()),
        None if lanes.is_empty() || heir.is_some() => Ok(()),
        None => {
            let named: Vec<String> = lanes.iter().map(|lane| format!("{} (branch {})", lane.ticket.display(), lane.branch)).collect();
            Err(NorthError::Usage(format!(
                "fleet forget {name}: queued land tickets still report to it and it has no parent or successor to take them: {}; forget it once they are decided, name one with --to NAME, or pass --force REASON",
                named.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::registry::fixtures::{fleet, live, live_under};
    use super::super::registry::{RemoteLane, RemoteLanes};
    use super::*;

    #[derive(Default)]
    struct Freed(Mutex<Vec<(String, PathBuf)>>);

    impl RemoteLanes for Freed {
        fn free(&self, lane: &RemoteLane) -> NorthResult<()> {
            self.0.lock().unwrap().push((lane.ssh.clone(), lane.path.clone()));
            Ok(())
        }
    }

    fn remote(fleet: &super::super::registry::Fleet, name: &str, ssh: &str, cwd: &Path, output: &str) {
        let dir = fleet.root.join(name);
        fs::create_dir_all(dir.join("g1")).unwrap();
        fs::write(dir.join("g1/stream.jsonl"), output).unwrap();
        let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": cwd, "model": null, "created_ms": 1, "host_pid": null, "conversation": name, "parent": null, "host_dir": dir, "stream_offset": 0, "machine": {"name": "gex45", "ssh": ssh}});
        fs::write(dir.join("session.json"), record.to_string()).unwrap();
    }

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

        let Err(NorthError::Usage(why)) = fleet.forget_unless_queued("lander-w33-failfast", None, None) else { panic!("a queued lane refuses the forget") };

        assert!(why.contains(&format!("{} (branch w33-concurrent)", path.display())), "{why}");
        assert!(why.contains("--force REASON"), "{why}");
        assert!(fleet.session("lander-w33-failfast").is_ok(), "a refused forget keeps the session");
    }

    #[test]
    fn a_retiring_session_hands_its_queued_lane_to_its_parent_and_is_refused_when_it_has_no_one_to_hand_it_to() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let lead = live(&fleet, "construct-lead");
        let _worker = live_under(&fleet, "n892-worker", Some("construct-lead"));
        let lane = ticket(home.path(), "", "1-n892-solo.json", "n892-worker", "/code/north/worktrees/n892-solo");

        fleet.retire("n892-worker", "reported", None).unwrap();

        assert_eq!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&lane).unwrap()).unwrap()["session"], "construct-lead");
        let frames = lead.frames.lock().unwrap();
        assert!(frames.iter().any(|frame| frame.contains("n892-worker is leaving the fleet") && frame.contains("queued lane w33-concurrent")), "{frames:?}");
        drop(frames);

        let _orphan = live(&fleet, "n903-orphan");
        let held = ticket(home.path(), "", "2-n903.json", "n903-orphan", "/code/north/worktrees/n903-waits");
        let Err(NorthError::Refused(why)) = fleet.retire("n903-orphan", "reported", None) else { panic!("a parentless session kept its lane unowned") };
        assert!(why.contains("no parent or successor") && why.contains("w33-concurrent"), "{why}");
        assert!(fleet.session("n903-orphan").is_ok());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(held).unwrap()).unwrap()["session"], "n903-orphan");
    }

    #[test]
    fn a_forget_naming_a_successor_hands_the_lane_to_it_even_without_a_parent() {
        let home = tempfile::tempdir().unwrap();
        let freed = Arc::new(Freed::default());
        let fleet = fleet(home.path()).freeing_lanes(freed.clone());
        let _worker = live(&fleet, "n903-orphan");
        let heir = live(&fleet, "n903-heir");
        let _child = live_under(&fleet, "n903-child", Some("n903-orphan"));
        let lane = ticket(home.path(), "", "1-n903.json", "n903-orphan", "/code/north/worktrees/n903-waits");

        let forgotten = fleet.forget_unless_queued("n903-orphan", None, Some("n903-heir")).unwrap();

        assert!(forgotten.overrode.is_empty());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(lane).unwrap()).unwrap()["session"], "n903-heir");
        assert!(heir.frames.lock().unwrap().iter().any(|frame| frame.contains("n903-orphan is leaving the fleet")));
        assert!(fleet.session("n903-orphan").is_err(), "the forget still ran");
        assert_eq!(fleet.record("n903-child").unwrap().parent.as_deref(), Some("n903-heir"), "forget --to left the forgotten session's live child with no live parent");
        assert!(fleet.forget_unless_queued("n903-heir", None, Some("nobody")).is_err());

        let main = Path::new("/code/north/main");
        let remote_lane = Path::new("/code/north/worktrees/n903-remote");
        let retired_lane = Path::new("/code/north/worktrees/n903-retired");
        remote(&fleet, "n903-remote", "tom@gex45", main, "cd /code/north/worktrees/SLUG\ncd /code/north/worktrees/n903-remote && cargo test\ngit -C /code/north/worktrees/n903-remote status\n");
        remote(&fleet, "n903-retired", "tom@cpx62", retired_lane, "");
        remote(&fleet, "n903-shared", "tom@gex45", main, "cd /code/north/worktrees/n903-busy\n");
        remote(&fleet, "n903-busy", "tom@gex45", Path::new("/code/north/worktrees/n903-busy"), "");
        fleet.forget("n903-remote").unwrap();
        fleet.retire("n903-retired", "reported", None).unwrap();
        fleet.forget("n903-shared").unwrap();
        assert_eq!(
            *freed.0.lock().unwrap(),
            [("tom@gex45".to_owned(), remote_lane.to_path_buf()), ("tom@cpx62".to_owned(), retired_lane.to_path_buf())],
            "a forgotten or retired remote session frees the lane it worked in on the machine that holds it, never the checkout it was launched in nor a lane another session there works in"
        );
    }
}
