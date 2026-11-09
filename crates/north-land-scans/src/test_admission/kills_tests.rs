use toml_edit::DocumentMut;

use std::path::Path;

use super::{fallbacks, judged_alone};

const HOSTS: &str = "[hosts.gex45]\nssh = \"tom@gex45\"\n\n[hosts.cpx62]\nssh = \"tom@cpx62\"\n\n[hosts.whiterabbit]\nreaches = [\"gex45\", \"cpx62\"]\n";

// KEEP(bug): on 2026-11-08 between 5:30 and 6:00 AM, 17 land checks batched onto cpx62 ran their mutant kill against .safe-push's landing.remote-hosts gex45, which cpx62 cannot reach ("ssh: Could not resolve hostname gex45"), so each reached no verdict 0.3 s in and at 9:25 AM, when choose offered only that unreachable gex45, safe-push on cpx62 could not queue a lane that adds a test (north#706, north#892, north#912)
#[test]
fn a_mutant_kill_inside_an_offload_slot_runs_on_the_slot_host_and_tries_only_hosts_it_reaches() {
    let cfg: DocumentMut = HOSTS.parse().unwrap();
    assert_eq!(fallbacks(true, || Some("gex45".to_owned()), "gex45", Some(&cfg), "cpx62"), ["cpx62"]);
    assert_eq!(fallbacks(false, || None, "gex45", Some(&cfg), "cpx62"), ["cpx62"], "on 2026-11-08 safe-push on cpx62 could not queue a lane that adds a test because cpx62 reaches no build host, so the kill scan runs on cpx62 itself (north#912)");
    assert_eq!(fallbacks(false, || None, "gex45", Some(&cfg), "whiterabbit"), ["gex45"]);
    let builds: DocumentMut = format!("{HOSTS}\n[hosts.cpx62b]\noffload_builds = true\n").parse().unwrap();
    assert_eq!(fallbacks(false, || None, "gex45", Some(&builds), "cpx62b"), ["cpx62b"], "on 2026-11-08 at 9:25 AM safe-push on cpx62 could not queue a lane that adds a test: choose found only gex45, which cpx62 cannot resolve, so the kill scan exited 75; a build host that reaches no other runs the scan itself");
}

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn lane(repo: &Path, path: &str, text: &str) -> String {
    std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
    std::fs::write(repo.join(path), text).unwrap();
    git_in(repo, &["add", path]);
    git_in(repo, &["commit", "-qm", path]);
    git_in(repo, &["rev-parse", "HEAD"])
}

// KEEP(bug): on 2026-11-08 in the hour to 8:52 AM every probe ran the mutant kill on its merged tree, 52 runs and 1573 s, though each lane's kill is judged against its own fork point when it queues (north#842)
#[test]
fn a_probe_judges_the_mutant_kill_on_its_merged_tree_only_when_two_of_its_lanes_touch_the_same_file() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let main = lane(repo, "README.md", "north\n");
    let first = lane(repo, "crates/a/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let second = lane(repo, "crates/b/src/lib.rs", "pub fn g() -> u8 { 2 }\n");
    let overlapping = lane(repo, "crates/a/src/lib.rs", "pub fn f() -> u8 { 3 }\n");
    let range = |head: &str| format!("{main}..{head}");
    assert_eq!(judged_alone(repo, &range(&second), &format!("{first} {second}")), Some(2));
    assert_eq!(judged_alone(repo, &range(&first), &first), Some(1));
    assert_eq!(judged_alone(repo, &range(&overlapping), &format!("{first} {second} {overlapping}")), None);
    assert_eq!(judged_alone(repo, &range(&second), ""), None);
    assert_eq!(judged_alone(repo, &range(&overlapping), &format!("{first} {second}")), None);
}
