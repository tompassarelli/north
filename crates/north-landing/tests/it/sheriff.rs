use std::collections::BTreeSet;
use std::path::Path;

use north_landing::land::sheriff;
use north_landing::landing::red_check::Failure;
use serde_json::{Value, json};

use super::serve::{Repository, World, git, report};

const CHECK: &str = "[ ! -e bad ]\n";

fn commit(repository: &Repository, file: &str, text: &str) -> String {
    std::fs::write(repository.checkout.join(file), text).unwrap();
    git(&repository.checkout, &["add", "-A"]);
    git(&repository.checkout, &["commit", "-q", "-m", &format!("land {file}")]);
    git(&repository.checkout, &["rev-parse", "HEAD"])
}

fn recorded(root: &Path) -> Vec<Value> {
    let mut files: Vec<_> = walk(root);
    files.sort();
    files.iter().flat_map(|file| std::fs::read_to_string(file).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect::<Vec<Value>>()).collect()
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().flat_map(|entry| if entry.path().is_dir() { walk(&entry.path()) } else { vec![entry.path()] }).collect()
}

fn names(origin: &Path) -> Vec<String> {
    git(origin, &["ls-tree", "--name-only", "refs/heads/main"]).lines().map(str::to_owned).collect()
}

#[test]
fn a_red_main_run_whose_previous_runs_were_not_green_reverts_exactly_the_culprit_records_the_confirmation_that_justified_it_and_lands_the_revert_before_the_queue() {
    let world = World::new();
    let north = world.repository("north");
    let green = commit(&north, "check.sh", CHECK);
    commit(&north, "a.txt", "a\n");
    commit(&north, "b.txt", "b\n");
    let culprit = commit(&north, "bad", "breaks check.sh\n");
    commit(&north, "d.txt", "d\n");
    let head = commit(&north, "e.txt", "e\n");
    git(&north.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    sheriff::file(&north.checkout, &head, &green, &BTreeSet::from([Failure::Harness("check.sh".to_owned())]), Path::new("post-land.log")).unwrap();
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
    let decisions = recorded(&world.path().join("state/north/decisions/land.sheriff"));
    let steps: Vec<&Value> = decisions.iter().map(|line| &line["decision"]).collect();
    assert_eq!(steps[steps.len() - 3..], [&json!({"confirm": 2}), &json!("recheck"), &json!({"revert": 2})], "the revert follows the confirmation and the recheck that justified it: {steps:?}");
    let case = &decisions.last().unwrap()["case"];
    assert_eq!(case["confirmations"], json!([[2, []]]), "main without the culprit passed check.sh: {case}");
    assert_eq!(case["lanes"][2], json!(culprit), "{case}");
}
