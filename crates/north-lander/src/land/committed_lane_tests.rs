use std::path::Path;
use std::process::Command;

use rusqlite::{Connection, params};

use super::*;

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(arguments).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").status().unwrap();
    assert!(status.success(), "git {arguments:?}");
}

#[test]
fn a_committed_lane_whose_session_ended_is_found_with_the_check_its_brief_named_until_handed_at_that_tip() {
    let root = tempfile::tempdir().unwrap();
    let (origin, main, lane) = (root.path().join("origin.git"), root.path().join("main"), root.path().join("worktrees/f-lane"));
    git(root.path(), &["init", "-q", "--bare", "-b", "main", &origin.to_string_lossy()]);
    git(root.path(), &["clone", "-q", &origin.to_string_lossy(), &main.to_string_lossy()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["push", "-q", "origin", "HEAD:main"]);
    git(&main, &["fetch", "-q", "origin"]);
    git(&main, &["worktree", "add", "-q", "--no-track", "-b", "f-lane", &lane.to_string_lossy(), "refs/remotes/origin/main"]);
    std::fs::write(lane.join("fix.rs"), "fn fixed() {}\n").unwrap();
    git(&lane, &["add", "fix.rs"]);
    git(&lane, &["commit", "-q", "-m", "Fix it"]);
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, fleet_name TEXT, role TEXT, started TEXT, lane TEXT, brief_text TEXT)").unwrap();
    let brief = "Item: north#1\nDone when: `cargo nextest run -p north-x -E 'test(/fix/)'` exits 0\n";
    db.execute("INSERT INTO runs VALUES ('a', 'w-old', 'worker', '2026-11-13T10:00:00Z', ?1, 'Done when: `true`')", params![lane.to_string_lossy()]).unwrap();
    db.execute("INSERT INTO runs VALUES ('b', 'w-1', 'worker', '2026-11-13T11:00:00Z', ?1, ?2)", params![lane.to_string_lossy(), brief]).unwrap();

    assert!(found(&db, |_| false, |_| false).is_empty(), "a lane whose session still runs is that session's to hand in");
    assert!(found(&db, |_| true, |_| true).is_empty(), "a lane a pending ticket names is already queued");
    let head = super::git(&lane, &["rev-parse", "HEAD"]).unwrap();
    let abandoned = found(&db, |session| session == "w-1", |_| false);
    assert_eq!(abandoned, [Abandoned { lane: lane.clone(), session: "w-1".into(), check: "cargo nextest run -p north-x -E 'test(/fix/)'".into(), head: head.clone() }]);
    std::fs::write(git_path(&lane, HANDED).unwrap(), format!("{head}\n")).unwrap();
    assert!(found(&db, |_| true, |_| false).is_empty(), "a tip the tick already handed is not handed again");
}
