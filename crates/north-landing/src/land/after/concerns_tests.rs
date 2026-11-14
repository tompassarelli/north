use std::path::Path;
use std::process::Command;

use super::{Landed, attributable, culprits, flips, queue, settled};
use crate::land::candidate::{self, git};
use crate::land::lander::Lander;

const CONCERNS: &str = r#"
[[concern]]
name = "no-bad-file"
source = "AGENTS.md: nothing named BAD"
oracle = "find . -maxdepth 1 -name BAD | wc -l"
unit = "files"
max = 0
class = "contract"
reads = "commit"

[[concern]]
name = "no-old-file"
source = "AGENTS.md: nothing named OLD"
oracle = "find . -maxdepth 1 -name OLD | wc -l"
max = 0
class = "contract"
reads = "commit"

[[concern]]
name = "no-wrong-file"
source = "AGENTS.md: nothing named WRONG, read by a tool that faults without its runtime"
oracle = "[ -e TOOL ] || { echo 'no whole runtime' >&2; exit 75; }; find . -maxdepth 1 -name WRONG | wc -l"
max = 0
class = "contract"
reads = "commit"

[[concern]]
name = "host-bad"
source = "AGENTS.md: a verdict this host recorded"
oracle = "find . -maxdepth 1 -name BAD | wc -l"
max = 0
class = "contract"
reads = "host"
"#;

fn commit(repo: &Path, files: &[&str]) -> String {
    for file in files {
        std::fs::write(repo.join(file), if *file == "concerns.toml" { CONCERNS } else { "x" }).unwrap();
        git(repo, &["add", file]).unwrap();
    }
    let status = Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", &format!("add {}", files.join(" "))]).status().unwrap();
    assert!(status.success());
    git(repo, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

#[test]
fn a_concern_reverts_only_its_own_flip_never_one_red_at_the_parent_one_whose_oracle_faults_or_one_that_reads_host_state() {
    let container = std::env::temp_dir().join(format!("north-concern-flip-{}", std::process::id()));
    let (repo, scratch, candidate, origin) = (container.join("main"), container.join("scratch"), container.join("scratch-candidate"), container.join("origin.git"));
    let _ = std::fs::remove_dir_all(&container);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]).unwrap();
    git(&repo, &["config", "user.name", "t"]).unwrap();
    git(&repo, &["config", "user.email", "t@t"]).unwrap();
    let base = commit(&repo, &["concerns.toml", "OLD"]);
    commit(&repo, &["one.txt"]);
    let culprit = commit(&repo, &["BAD", "TOOL", "WRONG"]);
    let head = commit(&repo, &["three.txt"]);
    git(&repo, &["worktree", "add", "-q", "--detach", scratch.to_str().unwrap(), &base]).unwrap();
    git(&repo, &["worktree", "add", "-q", "--detach", candidate.to_str().unwrap(), &base]).unwrap();
    let contracts = north_concerns::attribute::contracts(CONCERNS).unwrap();
    let red = settled(&repo, &contracts);
    assert_eq!(red.iter().map(|concern| concern.name.as_str()).collect::<Vec<_>>(), ["no-bad-file", "no-old-file", "no-wrong-file", "host-bad"]);
    let commit_reading = attributable(red, &head);
    let landed = Landed { project: "north", main: &repo, tree: &repo, scratch: &scratch, base: &base, head: &head, north: "/runtimes/abc/north" };
    let (reverted, refused) = flips(culprits(&landed, &commit_reading, &candidate).unwrap());
    assert_eq!(reverted.iter().map(|(commit, weighed)| (commit.clone(), weighed.iter().map(|weighed| weighed.concern.clone()).collect::<Vec<_>>())).collect::<Vec<_>>(), vec![(culprit.clone(), vec!["no-bad-file".to_owned()])], "only the landing that turned a concern red is reverted, and only for that concern");
    assert_eq!(refused.iter().map(|weighed| weighed.concern.as_str()).collect::<Vec<_>>(), ["no-wrong-file"], "a concern red at the parent is no flip, and an oracle that faults at the parent reverts nothing");
    assert!(refused[0].outputs().contains("no whole runtime"), "a refused revert keeps the oracle's own output: {}", refused[0].outputs());
    let already_red = Landed { base: &culprit, ..landed };
    let (reverted, refused) = flips(culprits(&already_red, &commit_reading, &candidate).unwrap());
    assert!(reverted.is_empty() && refused.is_empty(), "a concern red at both the parent and the landing reverts nothing");
    let lander = Lander { queue: container.join("queue"), ..Lander::new(&repo, Vec::new()).unwrap() };
    let landed = Landed { base: &base, ..already_red };
    queue(&landed, &lander, &culprit, "no-bad-file", "no-bad-file reads green (0) at the parent P and red (1) at C").unwrap();
    let message = git(&candidate::lander_worktree(&repo, "lander-concerns").unwrap(), &["log", "-1", "--format=%B", &format!("concerns-revert-{}", &culprit[..12])]).unwrap();
    assert!(message.contains(&format!("Reverts: {culprit}")) && message.contains("green (0) at the parent P and red (1) at C") && message.contains("/runtimes/abc/north"), "{message}");
    let queued = std::fs::read_dir(&lander.queue).unwrap().flatten().filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).count();
    assert_eq!(queued, 1, "one ticket for the revert goes ahead of the queue");
    assert!(queue(&landed, &lander, &culprit, "no-bad-file", "").unwrap().contains("already queued"));
    let _ = std::fs::remove_dir_all(&container);
}
