use std::fs;

use super::fixtures::{fleet, live};

fn works_in(fleet: &super::Fleet, name: &str, text: &str) {
    let stream = fleet.root.join(name).join("g1/stream.jsonl");
    let mut body = fs::read_to_string(&stream).unwrap();
    body.push_str(text);
    fs::write(stream, body).unwrap();
}

#[test]
fn a_running_session_works_in_the_worktree_its_recent_output_names_most_and_the_asker_is_left_out() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let _worker = live(&fleet, "worker");
    let _asker = live(&fleet, "asker");
    let _idle = live(&fleet, "idle");
    works_in(&fleet, "worker", "{\"file_path\":\"/c/north/worktrees/one/a.rs\"}\n{\"command\":\"cd /c/north/worktrees/one && ls /c/north/worktrees/two\"}\n");
    works_in(&fleet, "asker", "{\"file_path\":\"/c/north/worktrees/three/a.rs\"}\n");
    let lanes = fleet.running_lanes("/c/north/worktrees", "asker");
    assert_eq!(lanes.len(), 1);
    assert_eq!((lanes[0].session.as_str(), lanes[0].path.to_str()), ("worker", Some("/c/north/worktrees/one")));
    assert!(fleet.running_lanes("/c/wisp/worktrees", "asker").is_empty());
}
