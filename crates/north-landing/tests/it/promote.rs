use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_landing::land::queue::{self, Ticket};

const ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";

fn candidate_script(verdict: &str, during_replay: &str) -> String {
    format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n[ \"$1 $2\" = \"land replay\" ] && {{ {during_replay} echo 'replay: verdict {verdict}'; }}\nexit 0\n")
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

pub(super) struct Promoted {
    pub(super) status: bool,
    pub(super) stdout: String,
    pub(super) calls: String,
    pub(super) alarms: String,
}

fn dry_run(candidate_verdict: &str, lane_b_verdict: impl FnOnce(&Path) -> String) -> Promoted {
    dry_run_while(candidate_verdict, ":;", lane_b_verdict)
}

fn dry_run_while(candidate_verdict: &str, during_replay: &str, lane_b_verdict: impl FnOnce(&Path) -> String) -> Promoted {
    scenario(candidate_script(candidate_verdict, during_replay), None, lane_b_verdict)
}

pub(super) struct Held<'a> {
    pub(super) script: String,
    pub(super) change: Option<&'a str>,
    pub(super) held_for: Option<u64>,
}

pub(super) fn scenario(script: String, held: Option<Held>, lane_b_verdict: impl FnOnce(&Path) -> String) -> Promoted {
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
    std::fs::create_dir_all(checkout.join("crates/north-landing/src")).unwrap();
    std::fs::create_dir_all(checkout.join("crates/north-pace/src")).unwrap();
    std::fs::write(checkout.join("crates/north-landing/src/probe.rs"), "fn lander_probe() {}\n").unwrap();
    std::fs::write(checkout.join("crates/north-pace/src/probe.rs"), "fn pace_probe() {}\n").unwrap();
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
    let held_for = held.as_ref().and_then(|held| held.held_for);
    let (held_name, candidate_name) = match held.as_ref().and_then(|held| held.change) {
        Some(message) => (landed, commit("fix.txt", message)),
        None => ("held".to_owned(), "candidate".to_owned()),
    };
    let candidate = state.join("north/runtimes").join(&candidate_name).join("tree/bin/north");
    std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
    north_base::foundation::shim::write_executable(&candidate, script);
    std::os::unix::fs::symlink(format!("runtimes/{candidate_name}"), state.join("north/current")).unwrap();
    if let Some(held) = held {
        let binary = state.join("north/runtimes").join(&held_name).join("tree/bin/north");
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        north_base::foundation::shim::write_executable(&binary, held.script);
        std::os::unix::fs::symlink(format!("runtimes/{held_name}"), state.join("north/lander")).unwrap();
    }
    let alarmed = root.join("alarms.log");
    let north = root.join("bin/north");
    std::fs::create_dir_all(north.parent().unwrap()).unwrap();
    north_base::foundation::shim::write_executable(&north, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\n", alarmed.display()));
    if let Some(age) = held_for {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - age;
        std::fs::write(state.join("north/lander-held"), format!("{since}\n")).unwrap();
    }
    let mut arguments = vec!["promote", "--shadow-minutes", "0", "--repo", checkout.to_str().unwrap()];
    if held_for.is_none() {
        arguments.push("--dry-run");
    }
    let arguments = arguments.join("\n");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "serve::lander_process", "--nocapture", "--test-threads", "1"])
        .env(ARGUMENTS, arguments)
        .env("XDG_STATE_HOME", &state)
        .env("NORTH_LAND_NORTH", &north)
        .env("HOME", root)
        .env("GIT_CONFIG_GLOBAL", root.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let calls = std::fs::read_to_string(candidate.with_extension("calls")).unwrap_or_default();
    Promoted { status: output.status.success(), stdout, calls, alarms: std::fs::read_to_string(&alarmed).unwrap_or_default() }
}

pub(super) fn unfit_signature(log: &Path) -> String {
    format!("failed {}", log.display())
}

#[test]
fn promote_judges_each_recorded_round_against_the_candidate_and_leaves_the_link_when_it_differs_or_current_moves() {
    let unfit = dry_run("landed", unfit_signature);
    let stdout = &unfit.stdout;
    assert!(unfit.status, "{stdout}");
    assert!(stdout.contains("north land promote: round lane-b recorded failed on an unfit check host (error: failed to download `ahash v0.8.12`): not a verdict the candidate must reproduce"), "{stdout}");
    assert!(stdout.lines().any(|line| line.contains("round lane-a on ") && line.ends_with("recorded landed replayed landed: matches")), "{stdout}");
    assert!(stdout.contains("dry run: 1 round(s) matched;") && !stdout.contains("refused"), "{stdout}");
    assert_eq!(unfit.calls.lines().count(), 1, "only the landed round is replayed: {}", unfit.calls);
    assert!(unfit.calls.contains("lane-a") && !unfit.calls.contains("lane-b"), "{}", unfit.calls);

    let improved = dry_run("landed", |_| "failed lane commits missing from the candidate after the pick".to_owned());
    let stdout = &improved.stdout;
    assert!(improved.status, "{stdout}");
    assert!(stdout.lines().any(|line| line.contains("round lane-b on ") && line.ends_with("recorded failed but the candidate lands it: an improvement, not a hold")), "{stdout}");
    assert!(stdout.contains("dry run: 2 round(s) matched;"), "{stdout}");

    let refused = dry_run("failed", unfit_signature);
    let stdout = &refused.stdout;
    assert!(!refused.status, "{stdout}");
    assert!(stdout.contains("north land promote: refused: round lane-a on ") && stdout.contains("recorded landed but the candidate replayed failed; the lander link is unchanged"), "{stdout}");

    let moved = dry_run_while("landed", "ln -sfn runtimes/newer \"$XDG_STATE_HOME/north/current\";", unfit_signature);
    let stdout = &moved.stdout;
    assert!(!moved.status, "{stdout}");
    assert!(stdout.contains("refused: ") && stdout.contains("moved from runtimes/candidate to runtimes/newer while the candidate replayed; the lander link is unchanged"), "{stdout}");
}

pub(super) fn replay_script(verdict: &str, failures: &[&str]) -> String {
    let failing: String = failures.iter().map(|failure| format!("echo 'replay: failure cargo {failure}'; ")).collect();
    format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n[ \"$1 $2\" = \"land replay\" ] && {{ {failing}echo 'replay: verdict {verdict}'; }}\nexit 0\n")
}

const IN_LANDER: &str = "tests::lander_probe";
pub(super) const ELSEWHERE: &str = "tests::pace_probe";
const PRE_EXISTING: &str = "tests::capacity_agent_compile";
const REVERT: &str = "Revert \"Gate the capacity agent build\"";
const SHERIFF_FIX: &str = "Make main green again\n\nMain-red: #671";

fn compared(candidate: &[&str], held: &[&str], change: Option<&str>) -> Promoted {
    scenario(replay_script("failed", candidate), Some(Held { script: replay_script("failed", held), change, held_for: None }), unfit_signature)
}

#[test]
fn promote_follows_a_candidate_that_fails_nothing_the_held_runtime_does_not_and_holds_on_a_new_failure() {
    let same = compared(&[PRE_EXISTING], &[PRE_EXISTING, ELSEWHERE], None);
    assert!(same.status, "{}", same.stdout);
    assert!(same.stdout.contains("recorded landed but the candidate replayed failed: not a hold, the held runtime replays it the same way: no new failure"), "{}", same.stdout);
    assert!(same.stdout.contains("dry run: 1 round(s) matched;"), "{}", same.stdout);

    let elsewhere = compared(&[PRE_EXISTING, ELSEWHERE], &[PRE_EXISTING], None);
    assert!(!elsewhere.status, "{}", elsewhere.stdout);
    assert!(elsewhere.stdout.contains("recorded landed but the candidate replayed failed") && elsewhere.stdout.contains("new failures the held runtime does not have: tests::pace_probe"), "{}", elsewhere.stdout);

    let in_lander = compared(&[PRE_EXISTING, IN_LANDER], &[PRE_EXISTING], Some(REVERT));
    assert!(!in_lander.status, "{}", in_lander.stdout);
    assert!(in_lander.stdout.contains("new failures the held runtime does not have: tests::lander_probe"), "{}", in_lander.stdout);

    let revert = compared(&[PRE_EXISTING, ELSEWHERE], &[PRE_EXISTING], Some(REVERT));
    assert!(revert.status, "{}", revert.stdout);
    assert!(revert.stdout.contains("a revert or sheriff-fix candidate whose new failures (tests::pace_probe) lie outside the lander's crates"), "{}", revert.stdout);

    let fix = compared(&[ELSEWHERE], &[PRE_EXISTING], Some(SHERIFF_FIX));
    assert!(fix.status, "{}", fix.stdout);
    assert!(fix.stdout.contains("a revert or sheriff-fix candidate"), "{}", fix.stdout);

    let regressed = scenario(replay_script("failed", &[PRE_EXISTING]), Some(Held { script: replay_script("landed", &[]), change: None, held_for: None }), unfit_signature);
    assert!(!regressed.status, "{}", regressed.stdout);
    assert!(regressed.stdout.contains("the held runtime replayed landed"), "{}", regressed.stdout);
}
