use std::path::Path;

use super::*;
use north_base::platform::finish_record::Finished;
use north_base::platform::review_accept::Accept;

fn north_lists() -> &'static str {
    north_base::platform::harness::table(DEFAULTS)
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

#[test]
fn a_withdrawn_accept_no_longer_counts() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[("crates/north-lander/src/land.rs", "a\n"), (DEFAULTS, north_lists())], "base");
    let lane = commit(&repo, &[("crates/north-lander/src/land.rs", "b\n")], "inside the boundary");
    let test = commit(&repo, &[("crates/north-lander/tests/land.rs", "t\n")], "a test");
    accept(&accepts, &repo, &base, &lane, "reviewer-a");
    accept(&accepts, &repo, &base, &lane, "reviewer-b");
    assert_eq!(check(&repo, &base, &lane, &accepts, &[]), Ok(()));
    let patch = patch_id(&repo, &base, &lane).unwrap();
    assert_eq!(accepts.withdraw(&patch, "reviewer-b"), Ok(true));
    assert_eq!(check(&repo, &base, &lane, &accepts, &[]), Ok(()), "the other session's accept still counts");
    assert_eq!(accepts.withdraw(&patch, "reviewer-a"), Ok(true));
    assert!(check(&repo, &base, &lane, &accepts, &[]).unwrap_err().contains("holds no review accept"), "a withdrawn accept still counted");
    assert_eq!(accepts.withdraw(&patch, "reviewer-a"), Ok(false), "a second withdrawal finds nothing");

    accept(&accepts, &repo, &lane, &test, "reviewer-b");
    assert_eq!(check(&repo, &lane, &test, &accepts, &[]), Ok(()));
    assert_eq!(accepts.withdraw(&patch_id(&repo, &lane, &test).unwrap(), "reviewer-b"), Ok(true));
    assert!(check(&repo, &lane, &test, &accepts, &[]).is_err(), "a withdrawn accept still landed a one-accept change");

    let rules = commit(&repo, &[("harness/merge-sets.toml", "[[set]]\npath = \"src/cli/reference.txt\"\nkind = \"regenerate\"\ncommand = \"north docs reference-index\"\n\n[[set]]\npath = \"Cargo.lock\"\nkind = \"regenerate\"\ncommand = \"cargo update --workspace --offline\"\n")], "rules");
    let edited = commit(&repo, &[("crates/north-lander/src/land.rs", "c\n")], "edit");
    let regenerated = commit(&repo, &[("src/cli/reference.txt", "regenerated\n")], "the lander's regenerated index");
    assert_eq!(patch_id(&repo, &rules, &regenerated), patch_id(&repo, &rules, &edited), "a regenerated line the lander rewrites voided the lane's accepts (5985b7d80e)");
    let locked = commit(&repo, &[("Cargo.lock", "planted\n")], "a hand edit cargo update keeps");
    assert_ne!(patch_id(&repo, &rules, &locked), patch_id(&repo, &rules, &regenerated), "a hand edit of a file its generator does not wholly rewrite kept the lane's accepts");
}

fn commit(repo: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git_in(repo, &["add", "-A"]);
    git_in(repo, &["commit", "-qm", message]);
    git_in(repo, &["rev-parse", "HEAD"])
}

fn accept(accepts: &Accepts, repo: &Path, base: &str, head: &str, session: &str) {
    let fork = git::merge_base(repo, base, head);
    let patch_id = patch_id(repo, &fork, head).unwrap();
    accepts.record(&Accept { patch_id, session: session.to_owned(), at: 1 }).unwrap();
}

const LANDER: &str = "crates/north-lander/src/land.rs";

#[test]
fn a_live_fix_lane_that_changes_verify_north_login_with_no_review_accepts_is_refused_f9f0d1ddf9() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let files = ["README.md", "docs/rationale/0027-product-verification.md", "docs/onboarding.md", "flake.nix", "src/bin/verify_north/login.rs", "src/bin/verify_north/main.rs"];
    let base_files: Vec<(&str, &str)> = files.map(|file| (file, "a\n")).into_iter().chain([(DEFAULTS, north_lists())]).collect();
    let base = commit(&repo, &base_files, "base");
    let lane = commit(&repo, &files.map(|file| (file, "b\n")), "Make the Quickstart one command\n\nLive-fix: launch gate for north#1281");
    let refusal = check(&repo, &base, &lane, &accepts, &[]).unwrap_err();
    assert!(refusal.contains("holds no review accept") && refusal.contains("src/bin/verify_north/login.rs"), "{refusal}");
    accept(&accepts, &repo, &base, &lane, "reviewer-a");
    assert_eq!(check(&repo, &base, &lane, &accepts, &[]), Ok(()), "one accept lands a credential change, Live-fix or not");
}

#[test]
fn a_lane_that_only_removes_a_crate_lands_with_no_accept_and_one_that_adds_a_dependency_still_needs_one_north_1650() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let package = |name: &str, extra: &str| format!("[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n{extra}\n");
    let serde = "[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
    let base = commit(&repo, &[
        (DEFAULTS, north_lists()),
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n"),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n\n[dependencies]\nb = { path = \"../b\" }\nserde = \"1\"\n"),
        ("crates/b/Cargo.toml", "[package]\nname = \"b\"\n"),
        ("crates/b/src/lib.rs", "pub fn b() {}\n"),
        ("Cargo.lock", &format!("version = 4\n\n{}\n{}\n{serde}", package("a", "dependencies = [\"b\", \"serde\"]"), package("b", ""))),
    ], "base");

    git_in(&repo, &["rm", "-rq", "crates/b"]);
    let removal = commit(&repo, &[
        ("Cargo.toml", "[workspace]\nmembers = [\"crates/a\"]\n"),
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n\n[dependencies]\nserde = \"1\"\n"),
        ("Cargo.lock", &format!("version = 4\n\n{}\n{serde}", package("a", "dependencies = [\"serde\"]"))),
    ], "Remove crate b");
    assert_eq!(check(&repo, &base, &removal, &accepts, &[]), Ok(()), "a lane that deletes a crate and its manifest and lock entries waited on a reviewer");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    git_in(&repo, &["rm", "-rq", "crates/b"]);
    let widened = north_lists().replacen("\"north-clean-install.service\"]", "\"north-clean-install.service\", \"sshd.service\"]", 1);
    assert_ne!(widened, north_lists());
    let reach = commit(&repo, &[(DEFAULTS, &widened)], "Remove crate b and widen host_op_units");
    let refusal = check(&repo, &base, &reach, &accepts, &[]).unwrap_err();
    assert!(refusal.contains("key host_op_units"), "a removal lane that also widens a worker's reach landed with no reviewer: {refusal}");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let regex = "[[package]]\nname = \"regex\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
    let addition = commit(&repo, &[
        ("crates/a/Cargo.toml", "[package]\nname = \"a\"\n\n[dependencies]\nb = { path = \"../b\" }\nregex = \"1\"\nserde = \"1\"\n"),
        ("Cargo.lock", &format!("version = 4\n\n{}\n{}\n{regex}\n{serde}", package("a", "dependencies = [\"b\", \"regex\", \"serde\"]"), package("b", ""))),
    ], "Add regex");
    let refusal = check(&repo, &base, &addition, &accepts, &[]).unwrap_err();
    assert!(refusal.contains("crates/a/Cargo.toml adds dependencies.regex") && refusal.contains("Cargo.lock adds regex 1.0.0"), "{refusal}");
}

#[test]
fn a_lane_that_changes_a_trust_boundary_path_lands_on_one_review_accept_from_either_reviewer_session_north_1422() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let accepts = Accepts(root.path().join("accepts"));
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(LANDER, "a\n"), ("src/a.rs", "a\n"), (DEFAULTS, north_lists())], "base");

    let outside = commit(&repo, &[("src/a.rs", "b\n")], "outside the boundary");
    assert_eq!(check(&repo, &base, &outside, &accepts, &[]), Ok(()), "a lane outside trust_boundary_paths needs no accept");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let inside = commit(&repo, &[(LANDER, "b\n")], "inside the boundary");
    assert!(check(&repo, &base, &inside, &accepts, &[]).unwrap_err().contains("holds no review accept"));
    accept(&accepts, &repo, &base, &inside, "reviewer-b");
    assert_eq!(check(&repo, &base, &inside, &accepts, &[]), Ok(()), "one accept from either reviewer session lands it");

    git_in(&repo, &["commit", "-q", "--amend", "-m", "the same patch, reworded"]);
    let reworded = git_in(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(check(&repo, &base, &reworded, &accepts, &[]), Ok(()), "the accepts follow the patch, not the commit");

    let author = Finished { session: "author-x".to_owned(), command: String::new(), head: inside.clone(), passed: true, at: 1, refused: Vec::new() };
    let few = Accepts(root.path().join("few"));
    accept(&few, &repo, &base, &inside, "author-x");
    assert_eq!(check(&repo, &base, &inside, &few, &[]), Ok(()), "without a record of who wrote the lane, its author's accept counts");
    assert!(check(&repo, &base, &inside, &few, std::slice::from_ref(&author)).is_err(), "a session that handed in a commit of the lane is not its reviewer");
    assert!(check(&repo, &base, &reworded, &few, std::slice::from_ref(&author)).is_err(), "nor once the lander has rewritten the commit it handed in");

    git_in(&repo, &["reset", "-q", "--hard", &base]);
    let test = commit(&repo, &[("crates/north-lander/tests/land.rs", "t\n")], "a test");
    assert!(check(&repo, &base, &test, &accepts, &[]).is_err());
    accept(&accepts, &repo, &base, &test, "reviewer-a");
    assert_eq!(check(&repo, &base, &test, &accepts, &[]), Ok(()), "one accept lands a test-only change");

    git_in(&repo, &["reset", "-q", "--hard", &inside]);
    let revert = commit(&repo, &[(LANDER, "a\n")], &format!("Revert\n\nThis reverts commit {inside}."));
    assert!(check(&repo, &inside, &revert, &accepts, &[]).is_err());
    accept(&accepts, &repo, &inside, &revert, "reviewer-a");
    assert_eq!(check(&repo, &inside, &revert, &accepts, &[]), Ok(()), "one accept lands an exact revert");

    let south = root.path().join("south");
    std::fs::create_dir(&south).unwrap();
    git_in(&south, &["init", "-q", "-b", "main"]);
    let eval = "agents/decisions/harness/overhaul-2026-11-11/evals/run.sh";
    let own_lists = "[trust_boundary_paths]\nvalue = [\"agents/decisions/harness/overhaul-2026-11-11/evals/\"]\n";
    let south_base = commit(&south, &[(DEFAULTS, own_lists), (eval, "a\n")], "base");
    let script = commit(&south, &[(eval, "b\n")], "an eval script");
    assert!(check(&south, &south_base, &script, &accepts, &[]).unwrap_err().contains("holds no review accept"), "an eval script that the repository's own list names needs an accept");
    accept(&accepts, &south, &south_base, &script, "reviewer-a");
    assert_eq!(check(&south, &south_base, &script, &accepts, &[]), Ok(()), "one accept lands it under the repository's own list");
    ring_zero_touched_covers_reverted_commits_quoted_paths_defaults_keys_safe_push_and_ignores_base_merges(root.path());
}

fn ring_zero_touched_covers_reverted_commits_quoted_paths_defaults_keys_safe_push_and_ignores_base_merges(root: &Path) {
    let repo = root.join("ring0-repo");
    std::fs::create_dir(&repo).unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let defaults_base = format!("{}\n[ring0_knob]\nvalue = 1\nconsumers = ['crates/north-lander/src/lib.rs']\n", north_lists());
    let base = commit(&repo, &[(LANDER, "a\n"), ("crates/north-lander/src/lib.rs", "pub fn x() {}\n"), ("src/a.rs", "a\n"), (DEFAULTS, &defaults_base)], "base");

    git_in(&repo, &["checkout", "-q", "-b", "reverted-ring0", &base]);
    commit(&repo, &[("crates/north-worker-sandbox/src/worker_sandbox/grant.rs", "pub fn g() {}\n")], "touch sandbox");
    git_in(&repo, &["rm", "-q", "crates/north-worker-sandbox/src/worker_sandbox/grant.rs"]);
    git_in(&repo, &["commit", "-q", "-m", "revert sandbox"]);
    let reverted_head = commit(&repo, &[("src/a.rs", "b\n")], "plain edit");
    assert_eq!(ring_zero_touched(&repo, &base, &reverted_head), Ok(vec!["crates/north-worker-sandbox/src/worker_sandbox/grant.rs".to_owned()]));

    git_in(&repo, &["checkout", "-q", "-b", "quoted-ring0", &base]);
    let quoted = "crates/north-worker-sandbox/src/worker_sandbox/café \"quoted\".rs";
    let quoted_head = commit(&repo, &[(quoted, "pub fn q() {}\n")], "quoted ring-0 path");
    assert_eq!(ring_zero_touched(&repo, &base, &quoted_head), Ok(vec![quoted.to_owned()]));

    git_in(&repo, &["checkout", "-q", "-b", "defaults-reverted", &base]);
    let defaults_changed = defaults_base.replace("value = 1", "value = 2");
    commit(&repo, &[(DEFAULTS, &defaults_changed)], "flip ring-0 key");
    let defaults_reverted = commit(&repo, &[(DEFAULTS, &defaults_base), ("src/a.rs", "c\n")], "restore ring-0 key");
    let defaults_touched = ring_zero_touched(&repo, &base, &defaults_reverted).unwrap();
    assert!(defaults_touched.iter().any(|entry| entry.contains("key ring0_knob")), "{defaults_touched:?}");
    git_in(&repo, &["checkout", "-q", "-b", "treesame-merge", &base]);
    commit(&repo, &[("src/a.rs", "first-parent\n")], "first parent plain edit");
    git_in(&repo, &["merge", "-q", "--no-ff", "defaults-reverted", "-X", "theirs", "-m", "merge side branch with reverted defaults key"]);
    let treesame_head = git_in(&repo, &["rev-parse", "HEAD"]);
    let treesame_touched = ring_zero_touched(&repo, &base, &treesame_head).unwrap();
    assert!(treesame_touched.iter().any(|entry| entry.contains("key ring0_knob")), "{treesame_touched:?}");
    git_in(&repo, &["config", "log.showRoot", "false"]);
    git_in(&repo, &["checkout", "--orphan", "orphan-ring0"]);
    git_in(&repo, &["rm", "-rf", "-q", "."]);
    commit(&repo, &[("crates/north-worker-sandbox/src/worker_sandbox/grant.rs", "pub fn r() {}\n"), ("src/a.rs", "a\n")], "orphan root touches sandbox");
    git_in(&repo, &["rm", "-q", "crates/north-worker-sandbox/src/worker_sandbox/grant.rs"]);
    let orphan_head = commit(&repo, &[("src/a.rs", "b\n")], "orphan removes sandbox");
    assert!(ring_zero_touched(&repo, &base, &orphan_head).unwrap().iter().any(|path| path == "crates/north-worker-sandbox/src/worker_sandbox/grant.rs"));

    git_in(&repo, &["checkout", "-q", "main"]);
    let advanced_main = commit(&repo, &[(LANDER, "main-ring0\n"), (DEFAULTS, &defaults_changed)], "main advances ring-0");
    git_in(&repo, &["checkout", "-q", "-b", "worker-merges-main", &base]);
    commit(&repo, &[("src/a.rs", "worker\n")], "worker plain edit");
    git_in(&repo, &["merge", "-q", "--no-ff", &advanced_main, "-m", "merge main"]);
    let merged_head = git_in(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(ring_zero_touched(&repo, &advanced_main, &merged_head), Ok(Vec::new()), "merging base must not attribute base's ring-0 changes to the worker lane");

    let bare_lists = root.join("no-defaults");
    std::fs::create_dir(&bare_lists).unwrap();
    git_in(&bare_lists, &["init", "-q", "-b", "main"]);
    let no_defaults_base = commit(&bare_lists, &[(".safe-push", "[land]\n"), ("modules/a.nix", "{}\n")], "base");
    let nix_edit = commit(&bare_lists, &[("modules/a.nix", "{ a = 1; }\n")], "module edit");
    assert_eq!(ring_zero_touched(&bare_lists, &no_defaults_base, &nix_edit), Ok(Vec::new()));
    let safe_push_edit = commit(&bare_lists, &[(".safe-push", "[land]\n\tcheck = true\n")], "touch safe-push");
    assert_eq!(ring_zero_touched(&bare_lists, &no_defaults_base, &safe_push_edit), Ok(vec![".safe-push".to_owned()]));
}

#[test]
fn accepts_recorded_on_the_reviewers_host_count_on_a_build_host_lander() {
    use north_base::platform::records_mirror::{self, ROOT, STAMP, Records, Stamp};
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let base = commit(&repo, &[(LANDER, "a\n"), (DEFAULTS, north_lists())], "base");
    let inside = commit(&repo, &[(LANDER, "b\n")], "inside the boundary");

    let reviewers_state = root.path().join("reviewers-host");
    let recorded = Accepts::of(&[("XDG_STATE_HOME".to_owned(), reviewers_state.display().to_string())], root.path());
    accept(&recorded, &repo, &base, &inside, "reviewer-a");

    let build_state = root.path().join("build-host");
    let mirror = build_state.join(ROOT);
    for path in records_mirror::selected(&reviewers_state, std::time::UNIX_EPOCH + records_mirror::KEPT) {
        std::fs::create_dir_all(mirror.join(&path).parent().unwrap()).unwrap();
        std::fs::copy(reviewers_state.join(&path), mirror.join(&path)).unwrap();
    }
    std::fs::write(mirror.join(STAMP), serde_json::to_vec(&Stamp { host: "reviewers-host".to_owned(), state: reviewers_state, at: 1.0 }).unwrap()).unwrap();

    let counted = Accepts::counted(&Records::of(&build_state, "build-host"));
    assert_eq!(check(&repo, &base, &inside, &counted, &[]), Ok(()), "the accept recorded on the reviewers' host lands the lane on the build host");
}
