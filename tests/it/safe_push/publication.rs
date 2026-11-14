use std::path::Path;

use north_core::foundation::shim;

use super::{Fixture, Real, Run, Scratch, git, write};

const DENYLIST: &str = "title = \"safe-push test deny-list\"\n[[rules]]\nid = \"fake-private-term\"\ndescription = \"fake private identity term\"\nregex = '''zebra-quokka-7731'''\n";

#[test]
fn a_branch_push_to_a_public_origin_is_refused_before_any_check_and_names_the_private_route() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-q", "-c", "scratch-work"]);
    fixture.commit("work.txt", "unlanded work\n", "unlanded work");
    let before = fixture.remote_state();
    for branch in ["scratch/warm-lanes", "finish-overlap-2", "claude/land-work"] {
        let ran = fixture.run_with(&["--to", branch], &[("SAFE_PUSH_TEST_VISIBILITY", Path::new("PUBLIC"))]);
        ran.expect(false, &[&format!("origin is public, so it carries no branch beside main and only the lander pushes to it; NOT pushed {branch}."), "north capacity offload run --base HEAD --head HEAD", "push it to a private remote"]);
        assert!(!ran.output.contains("language gate") && !fixture.traced().contains("<language-gate>"), "a refused public push ran its checks: {}", ran.output);
        assert_eq!(fixture.remote_state(), before, "a branch push reached the public origin");
    }
}

#[test]
fn a_branch_landing_refuses_a_private_author_email_before_push() {
    let scratch = Scratch::new();
    let fixture = Real::new(&scratch, "email-to-main");
    let private_email = "someone@private.example";
    git(&fixture.repo, &["commit", "--amend", "--allow-empty", "-qm", "private author for main", &format!("--author=safe-push-test <{private_email}>")]);
    let before = fixture.remote_state();
    let observed = Run::new(&scratch, &fixture.repo, &["--dry-run", "--to", "main"]).output();
    let sha = git(&fixture.repo, &["rev-parse", "--short", "HEAD"]);
    assert_ne!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains(&format!("safe-push: commit {sha} author email {private_email} is not safe-push-test@example.invalid")), "{}", observed.output);
    assert!(observed.output.contains("git commit --amend --reset-author (or rebase --exec) before pushing; NOT pushed."), "{}", observed.output);
    assert_eq!(fixture.remote_state(), before, "private author email refusal mutated the remote");
}

#[test]
fn a_language_gate_pass_publishes_to_another_branch() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-q", "-c", "feature"]);
    fixture.git(&["commit", "--allow-empty", "-qm", "feature"]);
    fixture.run(&["--to", "other"]).expect(true, &[]);
    assert_eq!(git(&fixture.remote, &["rev-parse", "other"]), fixture.head(), "a language pass did not publish");
    let tracking = std::process::Command::new("git").current_dir(&fixture.repo).args(["rev-parse", "--verify", "@{upstream}"]).output().unwrap();
    assert!(!tracking.status.success(), "--to changed local branch tracking");
}

#[test]
fn a_failed_fetch_of_the_destination_is_refused_before_any_check_runs() {
    let fixture = Fixture::new();
    shim::write_script(&fixture.repo.join(".git/hooks/pre-push"), "cat >/dev/null\n");
    fixture.git(&["config", "remote.origin.pushurl", &fixture.remote.to_string_lossy()]);
    fixture.git(&["config", "remote.origin.url", &fixture.root.join("unreachable.git").to_string_lossy()]);
    let before = fixture.remote_state();
    let ran = fixture.run(&["--to", "main"]);
    ran.expect(false, &["safe-push: could not fetch origin/main; NOT pushed."]);
    assert!(!ran.output.contains("blocking check(s) failed") && !ran.output.contains("language gate"), "a failed fetch was reported as a gate refusal: {}", ran.output);
    assert!(!fixture.traced().contains("<language-gate>"), "a failed fetch reached the language gate");
    assert_eq!(fixture.remote_state(), before, "a failed fetch published");
}

#[test]
fn a_public_destination_runs_the_deny_list_scan_and_the_private_kind_judge_and_names_the_rule_file_and_south_destination_without_printing_the_match() {
    let fixture = Fixture::new();
    let denylist = fixture.root.join("denylist.toml");
    write(&denylist, DENYLIST);
    fixture.commit("identity.txt", "owner: zebra-quokka-7731\n", "add identity");
    let before = fixture.remote_state();
    let ran = fixture.run_with(&["--dry-run"], &[("SAFE_PUSH_DENYLIST", &denylist), ("SAFE_PUSH_TEST_VISIBILITY", Path::new("PUBLIC"))]);
    ran.expect(false, &["fake-private-term", "identity.txt"]);
    assert!(!ran.output.contains("zebra-quokka-7731"), "the deny-list refusal printed the matched value: {}", ran.output);
    assert_eq!(fixture.remote_state(), before, "the deny-list refusal mutated the remote");
    fixture.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let (kinds, south) = (fixture.root.join("kinds.toml"), fixture.root.join("south"));
    write(&kinds, "[[kind]]\nid = \"fixture-kind\"\ndescription = \"a fixture kind\"\n");
    std::fs::create_dir_all(south.join("main")).unwrap();
    git(&south.join("main"), &["init", "-q"]);
    write(&south.join("main/plan.md"), "an operator plan\n");
    git(&south.join("main"), &["add", "plan.md"]);
    git(&south.join("main"), &["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "operator"]);
    shim::write_script(&fixture.bin.join("claude"), "if grep -q walnut-lantern; then echo 'fixture-kind | walnut-lantern'; else echo NONE; fi\n");
    let (profiles, cache, north) = (fixture.root.join("profiles"), fixture.root.join("verdicts"), Path::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")));
    let login = fixture.root.join("home/.claude");
    write(&login.join(".credentials.json"), "{\"claudeAiOauth\":{}}");
    let environment = [
        ("SAFE_PUSH_DENYLIST", denylist.as_path()),
        ("SAFE_PUSH_SOUTH", south.as_path()),
        ("SAFE_PUSH_TEST_VISIBILITY", Path::new("PUBLIC")),
        ("NORTH_PRIVATE_KINDS", kinds.as_path()),
        ("NORTH_PROFILES", profiles.as_path()),
        ("NORTH_PRIVACY_CACHE", cache.as_path()),
        ("NORTH_BIN", north),
        ("CLAUDE_CONFIG_DIR", login.as_path()),
    ];
    fixture.commit("notes.md", "the walnut-lantern plan\n", "add notes");
    let before = fixture.remote_state();
    fixture.run_with(&["--dry-run"], &environment).expect(false, &["file it in south: fixture-kind", "NOT pushed"]);
    assert_eq!(fixture.remote_state(), before, "the private-kind refusal mutated the remote");

    fixture.git(&["reset", "-q", "--hard", "HEAD~1"]);
    fixture.commit("notes.md", "the plan, stated generically\n", "add generic notes");
    fixture.run_with(&["--dry-run"], &environment).expect(true, &[]);

    fixture.commit("more.md", "another plan\n", "add another note");
    shim::write_script(&fixture.bin.join("claude"), "exit 1\n");
    fixture.run_with(&["--dry-run"], &environment).expect(false, &["private-text judge could not run", "NOT pushed"]);

    fixture.commit("blank.md", "a third plan\n", "add a third note");
    shim::write_script(&fixture.bin.join("claude"), "echo\n");
    fixture.run_with(&["--dry-run"], &environment).expect(false, &["unreadable form", "NOT pushed"]);

    fixture.commit("keyed.md", "a fourth plan\n", "add a fourth note");
    shim::write_script(&fixture.bin.join("claude"), "echo NONE\n");
    let keyed: Vec<_> = environment.iter().copied().chain([("ANTHROPIC_API_KEY", Path::new("planted"))]).collect();
    fixture.run_with(&["--dry-run"], &keyed).expect(false, &["ANTHROPIC_API_KEY is set", "NOT pushed"]);
    fixture.run_with(&["--dry-run"], &environment).expect(true, &[]);

    shim::write_script(&fixture.bin.join("claude"), "if grep -q walnut-lantern; then echo 'fixture-kind | walnut-lantern'; else echo NONE; fi\n");
    fixture.commit("harness/merge-sets.toml", "[[set]]\npath = \"index.md\"\nkind = \"regenerate\"\ncommand = \"true\"\nsources = [\"notes/\"]\n", "declare a generated index");
    fixture.commit("index.md", "the walnut-lantern plan, as its source already says\n", "regenerate the index");
    fixture.run_with(&["--dry-run"], &environment).expect(true, &[]);
}
