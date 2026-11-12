use toml_edit::DocumentMut;

use std::path::Path;

use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use super::own_tree::OwnTree;
use super::{End, Tally, fallbacks, judge, judge_each, judged_alone};

const HOSTS: &str = "[hosts.gex45]\nssh = \"tom@gex45\"\n\n[hosts.cpx62]\nssh = \"tom@cpx62\"\n\n[hosts.whiterabbit]\nreaches = [\"gex45\", \"cpx62\"]\n";

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

#[test]
fn kills_judges_each_added_test_against_the_lane_that_added_it() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let safe_push = "[mutant]\n\tsource = \":(glob)crates/*/src/**/*.rs\"\n\tsource = \":(exclude,glob)**/*tests*.rs\"\n";
    lane(repo, ".safe-push", safe_push);
    lane(repo, "crates/a/src/lib_tests.rs", "use super::*;\n");
    lane(repo, "crates/b/src/lib.rs", "pub fn g() -> u8 { 2 }\n");
    let main = lane(repo, "crates/a/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    std::fs::write(repo.join("crates/a/src/lib.rs"), "pub fn f() -> u8 { 3 }\n").unwrap();
    std::fs::write(repo.join("crates/a/src/lib_tests.rs"), "use super::*;\n#[test]\nfn a_test_guards_f() {\n    assert_eq!(f(), 3);\n}\n").unwrap();
    git_in(repo, &["add", "."]);
    git_in(repo, &["commit", "-qm", "first lane"]);
    let first = git_in(repo, &["rev-parse", "HEAD"]);
    let second = lane(repo, "crates/b/src/lib.rs", "pub fn g() -> u8 { 4 }\n");
    let range = format!("{main}..{second}");
    let mut out = Vec::new();

    let mut judged = Vec::new();
    let code = judge_each(repo, &range, "", false, |lane, _| {
        judged.push((lane.names.iter().cloned().collect::<Vec<_>>(), lane.files.clone()));
        0
    }, &mut out);
    assert_eq!(code, Some(0));
    assert_eq!(judged.len(), 1, "the second lane adds no test, so no lane but the first is judged: {judged:?}");
    assert_eq!(judged[0].0, ["a_test_guards_f"]);
    assert!(judged[0].1.iter().all(|file| file.starts_with("crates/a/")), "the first lane's tests are judged against the first lane's diff only: {judged:?}");

    let mut by_tips = 0;
    judge_each(repo, &range, &format!("{first} {second}"), false, |_, _| { by_tips += 1; 0 }, &mut out);
    assert_eq!(by_tips, 1, "lane tips split the range the same way");

    let work = root.path().join("work");
    std::fs::create_dir_all(work.join("mutants.out")).unwrap();
    std::fs::write(work.join("mutants.out/missed.txt"), "crates/a/src/lib.rs:1:5: replace f -> u8 with 0\n").unwrap();
    let survived = judge_each(repo, &range, "", false, |lane, out| judge(&work, lane, &End::Exited(ExitStatus::from_raw(0)), Tally { total: 1, cap: 900, seconds: 1 }, out), &mut out);
    assert_eq!(survived, Some(1), "a lane whose own added tests kill none of its own mutants still fails");
    assert!(String::from_utf8_lossy(&out).contains("a_test_guards_f survive all 1 mutant(s)"));

    let long_ago = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    let built = repo.join("target/debug");
    std::fs::create_dir_all(built.join("deps")).unwrap();
    std::fs::create_dir_all(built.join(".fingerprint/a")).unwrap();
    for (path, text) in [("deps/liba.rlib", "built\n"), ("deps/a.d", "inputs\n"), (".fingerprint/a/lib-a", "fresh\n")] {
        std::fs::write(built.join(path), text).unwrap();
    }
    std::fs::File::options().write(true).open(repo.join("crates/b/src/lib.rs")).unwrap().set_modified(long_ago).unwrap();
    let before = snapshot(repo);
    let tree = OwnTree::of(repo, &second).unwrap();
    let own = tree.root().to_path_buf();
    let modified = |path: &Path| std::fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(modified(&own.join("crates/b/src/lib.rs")), long_ago, "the sweep's tree keeps the check's source times, so cargo finds the check's build fresh");
    assert_eq!(modified(&own.join("target/debug/.fingerprint/a/lib-a")), modified(&built.join(".fingerprint/a/lib-a")));
    std::fs::write(own.join("crates/b/src/lib.rs"), "pub fn g() -> u8 { 0 }\n").unwrap();
    std::fs::write(own.join("target/debug/.fingerprint/a/lib-a"), "stale\n").unwrap();
    std::fs::write(own.join("target/debug/deps/a.d"), "mutated\n").unwrap();
    std::fs::write(own.join("target/debug/deps/liba.rlib.new"), "mutant\n").unwrap();
    std::fs::rename(own.join("target/debug/deps/liba.rlib.new"), own.join("target/debug/deps/liba.rlib")).unwrap();
    assert!(tree.written_through().is_empty());
    drop(tree);
    assert!(!own.exists());
    assert_eq!(snapshot(repo), before, "on 2026-11-12 a sweep mutated and rebuilt in place in the slot its check built, which another run then took; a sweep now writes nothing into the checkout or build directory of its check (north#1254)");
}

fn snapshot(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap().map(Result::unwrap).filter(|entry| entry.file_name() != ".git") {
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                found.push((entry.path(), std::fs::read(entry.path()).unwrap(), meta.modified().unwrap()));
            }
        }
    }
    found.sort();
    found
}
