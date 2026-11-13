use std::fs;
use std::path::{Path, PathBuf};

use north_base::foundation::clock::Timestamp;
use north_base::platform::land_queue;

use super::{Inputs, run_in};
use north_base::test_inputs::git::{git_in, init_repo};

fn commit(dir: &Path, file: &str, text: &str, subject: &str) -> String {
    fs::write(dir.join(file), text).unwrap();
    git_in(dir, &["add", file]);
    git_in(dir, &["commit", "-qm", subject]);
    git_in(dir, &["rev-parse", "HEAD"])
}

fn lane(main: &Path, name: &str) -> (PathBuf, String) {
    let path = main.parent().unwrap().join("worktrees").join(name);
    git_in(main, &["worktree", "add", "-q", "-b", name, path.to_str().unwrap(), "origin/main"]);
    commit(&path, &format!("{name}-a.txt"), "first\n", &format!("{name}: first half"));
    let head = commit(&path, &format!("{name}-b.txt"), "second\n", &format!("{name}: second half"));
    (path, head)
}

#[test]
fn a_lane_the_lander_squash_landed_from_this_host_is_retired_while_a_moved_or_dirty_one_stays() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let seed = home.join("seed");
    init_repo(&seed, None);
    let origin = home.join("origin.git");
    git_in(&home, &["clone", "-q", "--bare", seed.to_str().unwrap(), origin.to_str().unwrap()]);
    let main = home.join("code/proj/main");
    git_in(&home, &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    let lanes = ["landed", "moved", "dirty"].map(|name| (name, lane(&main, name)));
    let queue = land_queue::directory(&home.join(".local/state"), &git_in(&main, &["remote", "get-url", "origin"]));
    fs::create_dir_all(queue.join("done")).unwrap();
    for (name, (path, head)) in &lanes {
        fs::write(main.join(format!("{name}-a.txt")), "first\n").unwrap();
        let squashed = commit(&main, &format!("{name}-b.txt"), "second\n", &format!("Land the {name} lane (proj#1)"));
        let ticket = serde_json::json!({ "host": land_queue::local_host(), "root": path, "branch": name, "head": head, "verdict": format!("landed {squashed}") });
        fs::write(queue.join("done").join(format!("1-{name}.json")), ticket.to_string()).unwrap();
    }
    for (name, _) in &lanes {
        commit(&main, &format!("{name}-a.txt"), "rewritten on main\n", &format!("Rewrite what the {name} lane added"));
    }
    git_in(&main, &["push", "-q", "origin", "main"]);
    let (moved, dirty) = (&lanes[1].1.0, &lanes[2].1.0);
    commit(moved, "moved-c.txt", "after the ticket\n", "moved: more work after landing");
    fs::write(dirty.join("notes.txt"), "untracked work\n").unwrap();
    fs::create_dir_all(home.join("proc")).unwrap();

    let inputs = Inputs { fleet: Some(Vec::new()), now_ms: Timestamp::now().unix_millis() + 10 * 3_600_000, stale: home.join("stale.json"), proc_root: home.join("proc") };
    let arguments = ["--root", home.join("code").to_str().unwrap(), "--archive", home.join("archive").to_str().unwrap()].map(str::to_owned);
    assert_eq!(run_in(&home, inputs, &arguments), 0);

    assert!(!lanes[0].1.0.exists(), "a clean lane whose head a landed ticket from this host names is retired");
    assert!(git_in(&main, &["branch", "--list", "landed"]).is_empty());
    assert!(moved.exists(), "a lane whose head moved after its ticket stays");
    assert!(dirty.join("notes.txt").exists(), "a lane with untracked work stays");
}
