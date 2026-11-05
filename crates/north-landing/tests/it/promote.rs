use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_landing::land::queue::{self, Ticket};

const ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";
const CANDIDATE: &str = "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n[ \"$1 $2\" = \"land replay\" ] && echo 'replay: verdict landed'\nexit 0\n";
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

#[test]
fn promote_does_not_ask_the_candidate_to_reproduce_a_round_recorded_failed_on_an_unfit_check_host() {
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
    decided(&done, 2, &ticket("lane-b", &unfit, format!("failed {}", log.display())));
    let candidate = state.join("north/runtimes/candidate/tree/bin/north");
    std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
    north_base::foundation::shim::write_executable(&candidate, CANDIDATE);
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
    assert!(output.status.success(), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("north land promote: round lane-b recorded failed on an unfit check host (error: failed to download `ahash v0.8.12`): not a verdict the candidate must reproduce"), "{stdout}");
    assert!(stdout.lines().any(|line| line.contains("round lane-a on ") && line.ends_with("recorded landed replayed landed: matches")), "{stdout}");
    assert!(stdout.contains("dry run: 1 round(s) matched;"), "{stdout}");
    assert!(!stdout.contains("refused"), "{stdout}");
    let replays = std::fs::read_to_string(candidate.with_extension("calls")).unwrap();
    assert_eq!(replays.lines().count(), 1, "only the landed round is replayed: {replays}");
    assert!(replays.contains("lane-a") && !replays.contains("lane-b"), "{replays}");
}
