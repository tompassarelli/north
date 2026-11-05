use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_landing::land::queue::{self, Ticket};

const ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";

fn candidate_script(verdict: &str) -> String {
    format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n[ \"$1 $2\" = \"land replay\" ] && echo 'replay: verdict {verdict}'\nexit 0\n")
}

const UNFIT_LOG: &str = "safe-push: land check (inferred): cargo clippy --all-targets --locked --offline -- -D warnings\nerror: failed to download `ahash v0.8.12`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\nsafe-push: land check cargo clippy exited 101; log: /tmp/land-check.log; NOT pushed.\nsafe-push: verdict refused\n";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).env("GIT_CONFIG_GLOBAL", directory.ancestors().find(|path| path.join("gitconfig").is_file()).unwrap().join("gitconfig")).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(output.status.success(), "git {arguments:?} in {}: {}", directory.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn decided(done: &Path, micros: u128, ticket: &Ticket) -> PathBuf {
    let path = done.join(queue::file_name(micros, &ticket.branch));
    queue::write(&path, ticket).unwrap();
    path
}

struct Promoted {
    status: bool,
    stdout: String,
    calls: String,
}

fn dry_run(candidate_verdict: &str, lane_b_verdict: impl FnOnce(&Path) -> String) -> Promoted {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::write(root.join("gitconfig"), "[user]\n\tname = t\n\temail = t@example.com\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n").unwrap();
    let origin = root.join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    git(root, &["clone", "-q", origin.to_str().unwrap(), "main"]);
    let checkout = root.join("main");
    let commit = |file: &str, message: &str| {
        std::fs::write(checkout.join(file), format!("{message}\n")).unwrap();
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", message]);
        git(&checkout, &["rev-parse", "HEAD"])
    };
    let base = commit("README", "start");
    let unfit = commit("b.txt", "lane b (north#360)");
    git(&checkout, &["reset", "-q", "--hard", &base]);
    let landed = commit("a.txt", "lane a (north#360)");
    git(&checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let state = root.join("state");
    let done = queue::directory(&state, &git(&checkout, &["remote", "get-url", "--push", "origin"])).join("done");
    let log = root.join("logs/lane-b-precheck.log");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, UNFIT_LOG).unwrap();
    let ticket = |branch: &str, head: &str, verdict: String| Ticket { origin: origin.display().to_string(), root: checkout.clone(), branch: branch.to_owned(), head: head.to_owned(), decided_on: Some(base.clone()), verdict: Some(verdict), ..Default::default() };
    decided(&done, 1, &ticket("lane-a", &landed, format!("landed {landed}")));
    decided(&done, 2, &ticket("lane-b", &unfit, lane_b_verdict(&log)));
    let candidate = state.join("north/runtimes/candidate/tree/bin/north");
    std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
    north_base::foundation::shim::write_executable(&candidate, candidate_script(candidate_verdict));
    std::os::unix::fs::symlink("runtimes/candidate", state.join("north/current")).unwrap();
    let arguments = ["promote", "--dry-run", "--shadow-minutes", "0", "--repo", checkout.to_str().unwrap()].join("\n");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "serve::lander_process", "--nocapture", "--test-threads", "1"])
        .env(ARGUMENTS, arguments)
        .env("XDG_STATE_HOME", &state)
        .env("HOME", root)
        .env("GIT_CONFIG_GLOBAL", root.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let calls = std::fs::read_to_string(candidate.with_extension("calls")).unwrap_or_default();
    Promoted { status: output.status.success(), stdout, calls }
}

fn unfit_signature(log: &Path) -> String {
    format!("failed {}", log.display())
}

#[test]
fn promote_does_not_ask_the_candidate_to_reproduce_a_round_recorded_failed_on_an_unfit_check_host() {
    let promoted = dry_run("landed", unfit_signature);
    let stdout = &promoted.stdout;
    assert!(promoted.status, "{stdout}");
    assert!(stdout.contains("north land promote: round lane-b recorded failed on an unfit check host (error: failed to download `ahash v0.8.12`): not a verdict the candidate must reproduce"), "{stdout}");
    assert!(stdout.lines().any(|line| line.contains("round lane-a on ") && line.ends_with("recorded landed replayed landed: matches")), "{stdout}");
    assert!(stdout.contains("dry run: 1 round(s) matched;"), "{stdout}");
    assert!(!stdout.contains("refused"), "{stdout}");
    assert_eq!(promoted.calls.lines().count(), 1, "only the landed round is replayed: {}", promoted.calls);
    assert!(promoted.calls.contains("lane-a") && !promoted.calls.contains("lane-b"), "{}", promoted.calls);
}

#[test]
fn promote_counts_a_round_the_candidate_lands_that_the_recorded_lander_refused_as_an_improvement() {
    let promoted = dry_run("landed", |_| "failed lane commits missing from the candidate after the pick".to_owned());
    let stdout = &promoted.stdout;
    assert!(promoted.status, "{stdout}");
    assert!(stdout.lines().any(|line| line.contains("round lane-b on ") && line.ends_with("recorded failed but the candidate lands it: an improvement, not a hold")), "{stdout}");
    assert!(stdout.contains("dry run: 2 round(s) matched;"), "{stdout}");
}

#[test]
fn promote_holds_the_lander_link_naming_the_round_when_the_candidate_refuses_a_lane_the_lander_landed() {
    let promoted = dry_run("failed", unfit_signature);
    let stdout = &promoted.stdout;
    assert!(!promoted.status, "{stdout}");
    assert!(stdout.contains("north land promote: refused: round lane-a on ") && stdout.contains("recorded landed but the candidate replayed failed; the lander link is unchanged"), "{stdout}");
}
