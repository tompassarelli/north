use std::path::Path;
use std::process::Command;

use super::{Landed, culprits, queue, settled};
use crate::land::lander::Lander;
use crate::land::candidate::git;

const GOALS: &str = "[[goal]]\nname = \"no-bad-file\"\nsource = \"AGENTS.md: nothing named BAD\"\noracle = \"ls BAD 2>/dev/null | wc -l\"\nunit = \"files\"\nmax = 0\nclass = \"contract\"\n";

fn commit(repo: &Path, file: &str, text: &str) -> String {
    std::fs::write(repo.join(file), text).unwrap();
    git(repo, &["add", file]).unwrap();
    let status = Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", &format!("add {file}")]).status().unwrap();
    assert!(status.success());
    git(repo, &["rev-parse", "HEAD"]).unwrap()
}

#[test]
fn the_commit_that_turned_a_contract_goal_red_is_named_among_a_landed_range_and_a_goal_red_before_it_is_not() {
    let container = std::env::temp_dir().join(format!("north-goal-culprit-{}", std::process::id()));
    let (repo, scratch, origin) = (container.join("main"), container.join("scratch"), container.join("origin.git"));
    let _ = std::fs::remove_dir_all(&container);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]).unwrap();
    git(&repo, &["config", "user.name", "t"]).unwrap();
    git(&repo, &["config", "user.email", "t@t"]).unwrap();
    let base = commit(&repo, "goals.toml", GOALS);
    commit(&repo, "one.txt", "1");
    let culprit = commit(&repo, "BAD", "x");
    let head = commit(&repo, "three.txt", "3");
    git(&repo, &["worktree", "add", "-q", "--detach", scratch.to_str().unwrap(), &base]).unwrap();
    let red = settled(&repo).unwrap();
    assert_eq!(red.iter().map(String::as_str).collect::<Vec<_>>(), ["no-bad-file"]);
    let landed = Landed { project: "north", main: &repo, tree: &repo, scratch: &scratch, base: &base, head: &head };
    let found = culprits(&landed, red.clone()).unwrap();
    assert_eq!(found.into_iter().collect::<Vec<_>>(), vec![(culprit.clone(), vec!["no-bad-file".to_owned()])]);
    let already_red = Landed { base: &culprit, ..landed };
    assert!(culprits(&already_red, red).unwrap().is_empty(), "a goal red at the base of the landing is not this landing's fault");
    let lander = Lander { queue: container.join("queue"), ..Lander::new(&repo, Vec::new(), 1).unwrap() };
    let landed = Landed { base: &base, ..already_red };
    queue(&landed, &lander, &culprit, "no-bad-file").unwrap();
    let message = git(&repo, &["log", "-1", "--format=%B", &format!("goals-revert-{}", &culprit[..12])]).unwrap();
    assert!(message.contains(&format!("Reverts: {culprit}")) && message.contains("Goal-red: no-bad-file"), "{message}");
    let queued = std::fs::read_dir(&lander.queue).unwrap().flatten().filter(|entry| entry.path().is_file()).count();
    assert_eq!(queued, 1, "one ticket for the revert goes ahead of the queue");
    assert!(queue(&landed, &lander, &culprit, "no-bad-file").unwrap().contains("already queued"));
    let _ = std::fs::remove_dir_all(&container);
}
