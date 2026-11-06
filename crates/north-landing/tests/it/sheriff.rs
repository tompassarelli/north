use std::path::Path;

use super::serve::{Repository, World, git, report};

const CHECK: &str = "[ ! -e bad ]\n";

fn commit(repository: &Repository, file: &str, text: &str) -> String {
    std::fs::write(repository.checkout.join(file), text).unwrap();
    git(&repository.checkout, &["add", "-A"]);
    git(&repository.checkout, &["commit", "-q", "-m", &format!("land {file}")]);
    git(&repository.checkout, &["rev-parse", "HEAD"])
}

fn names(origin: &Path) -> Vec<String> {
    git(origin, &["ls-tree", "--name-only", "refs/heads/main"]).lines().map(str::to_owned).collect()
}

#[test]
fn a_red_main_run_whose_previous_runs_were_not_green_reverts_exactly_the_culprit_and_lands_the_revert_before_the_queue() {
    let world = World::new();
    let north = world.repository("north");
    let green = commit(&north, "check.sh", CHECK);
    commit(&north, "a.txt", "a\n");
    commit(&north, "b.txt", "b\n");
    let culprit = commit(&north, "bad", "breaks check.sh\n");
    commit(&north, "d.txt", "d\n");
    let head = commit(&north, "e.txt", "e\n");
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let message = format!("Main red at {head}\n\ntest: Check\nland check: check.sh exited 1\n\nGreen-parent: {green}\nMain-red-run: https://example.invalid/runs/9");
    let request = git(&north.checkout, &["commit-tree", &format!("{head}^{{tree}}"), "-p", &head, "-m", &message]);
    git(&north.checkout, &["push", "-q", "origin", &format!("{request}:refs/heads/claude/land-revert-{head}")]);
    north.lane("north-lane");
    git(&north.checkout.parent().unwrap().join("worktrees/north-lane"), &["push", "-q", "origin", "north-lane"]);
    let landers = [world.lander(&north, None).wait_with_output().unwrap(), world.lander(&north, None).wait_with_output().unwrap()];
    let transcript = landers.iter().map(report).collect::<Vec<_>>().join("\n");
    println!("{transcript}");
    let log = git(&north.origin, &["log", "--first-parent", "--format=%H %s", &format!("{head}..refs/heads/main")]);
    let landed: Vec<&str> = log.lines().collect();
    assert_eq!(landed.len(), 2, "{log}\n{transcript}");
    assert!(landed[0].ends_with("north-lane (north#360)"), "{log}\n{transcript}");
    assert!(landed[1].contains(" Revert land bad"), "{log}\n{transcript}");
    let revert = landed[1].split(' ').next().unwrap();
    let body = git(&north.origin, &["log", "-1", "--format=%B", revert]);
    assert!(body.contains(&format!("Sheriff-Reverts: {culprit}")), "{body}");
    assert_eq!(git(&north.origin, &["rev-parse", &format!("{revert}^")]), head);
    let files = names(&north.origin);
    assert!(!files.contains(&"bad".to_owned()), "{files:?}");
    for kept in ["a.txt", "b.txt", "d.txt", "e.txt", "check.sh"] {
        assert!(files.contains(&kept.to_owned()), "{kept} missing from {files:?}");
    }
    assert_eq!(git(&north.origin, &["for-each-ref", "refs/heads/claude/land-revert-"]).len(), 0);
    assert_eq!(git(&north.origin, &["for-each-ref", "refs/heads/north-lane"]), "", "a landing deletes the lane's origin branch\n{transcript}");
}
