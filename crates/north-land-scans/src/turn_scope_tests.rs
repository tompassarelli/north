use std::path::Path;

use super::*;
use crate::host_cases;

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

fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", "c"]);
    git_in(repo, &["rev-parse", "HEAD"])
}

const ONE: &str = "[[turn]]\ndate = 2026-11-12\n";
const TWO: &str = "[[turn]]\ndate = 2026-11-12\n\n[[turn]]\ndate = 2026-11-13\n";
const BARS: &str = "[[experiment]]\nbar = 1\n";
const MORE_BARS: &str = "[[experiment]]\nbar = 1\n\n[[experiment]]\nbar = 2\n";

#[test]
fn a_lane_outside_a_turns_scope_is_recorded_as_a_would_be_refusal_and_one_inside_it_is_not_and_a_host_case_beside_code_is_refused_one_alone_is_flagged_for_review_host_cases() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let base = commit(repo, &[(TURNS, ONE), (EXPERIMENTS, BARS), ("src/a.rs", "a\n")]);

    let inside = commit(repo, &[(TURNS, TWO), (EXPERIMENTS, MORE_BARS)]);
    assert_eq!(check(repo, &base, &inside), Ok(()));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let code = commit(repo, &[(TURNS, TWO), ("src/a.rs", "b\n")]);
    assert!(check(repo, &base, &code).unwrap_err().contains("also changes src/a.rs"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let moved = commit(repo, &[(TURNS, TWO), (EXPERIMENTS, "[[experiment]]\nbar = 9\n")]);
    assert!(check(repo, &base, &moved).unwrap_err().contains("so a bar, window or review day may move"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let rewritten = commit(repo, &[(TURNS, "[[turn]]\ndate = 2026-11-11\n")]);
    assert!(check(repo, &base, &rewritten).unwrap_err().contains("changes or removes a recorded [[turn]]"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let other = commit(repo, &[("src/a.rs", "c\n")]);
    assert_eq!(check(repo, &base, &other), Ok(()));
    let state = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    assert_eq!(scan(repo, &format!("{base}..{code}"), state.path(), &mut out), 0);
    assert!(String::from_utf8(out).unwrap().contains("would refuse (shadow)"));
    let log = std::fs::read_to_string(state.path().join("north/gates/turn-scope.jsonl")).unwrap();
    assert!(log.contains("\"would_refuse\":true"), "{log}");

    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git_in(repo, &["init", "-q", "-b", "main"]);
    let manifest = "crates/alpha/Cargo.toml";
    let plain = "[package]\nname = \"alpha\"\n";
    let with_case = "[package]\nname = \"alpha\"\n\n[[package.metadata.north.buck.host-cases]]\nname = \"t::a\"\nreason = \"needs openat2\"\n";
    let inline_case = "[package]\nname = \"alpha\"\nmetadata.north.buck.host-cases = [{ name = \"t::b\", reason = \"x\" }]\n";
    let base = commit(repo, &[(manifest, plain), ("crates/alpha/src/lib.rs", "a\n")]);

    let mixed = commit(repo, &[(manifest, with_case), ("crates/alpha/src/lib.rs", "b\n")]);
    let refusal = host_cases::check(repo, &base, &mixed).unwrap_err();
    assert!(refusal.contains("crate alpha adds or changes host case `t::a` in crates/alpha/Cargo.toml"), "{refusal}");

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let inline = commit(repo, &[(manifest, inline_case), ("crates/alpha/src/lib.rs", "b\n")]);
    assert!(host_cases::check(repo, &base, &inline).unwrap_err().contains("host case `t::b`"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let vendored = commit(repo, &[(manifest, with_case), ("build/host-cases.list", "alpha t::a\n"), ("third-party/BUCK", "edited\n")]);
    assert!(host_cases::check(repo, &base, &vendored).unwrap_err().contains("the lane also changes third-party/BUCK"));

    git_in(repo, &["reset", "-q", "--hard", &base]);
    let alone = commit(repo, &[(manifest, with_case), ("build/host-cases.list", "alpha t::a\n"), ("crates/alpha/BUCK", "skip\n")]);
    assert_eq!(host_cases::check(repo, &base, &alone), Ok(true));
    let mut out = Vec::new();
    assert_eq!(host_cases::scan(repo, &format!("{base}..{alone}"), state.path(), &mut out), 0);
    assert_eq!(String::from_utf8(out).unwrap(), "review: host-case change\n");

    let kept = commit(repo, &[("crates/alpha/src/lib.rs", "c\n")]);
    assert_eq!(host_cases::check(repo, &alone, &kept), Ok(false));
    let removed = commit(repo, &[(manifest, plain), ("crates/alpha/src/lib.rs", "d\n")]);
    assert_eq!(host_cases::check(repo, &kept, &removed), Ok(false));

    let mut out = Vec::new();
    assert_eq!(host_cases::scan(repo, &format!("{base}..{mixed}"), state.path(), &mut out), 0);
    assert!(String::from_utf8(out).unwrap().contains("would refuse (shadow)"));
}
