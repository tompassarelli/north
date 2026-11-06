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
    pub(super) candidate: String,
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
    pub(super) ahead: bool,
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
        None => ("held".to_owned(), commit("candidate.txt", "candidate")),
    };
    let newer = commit("newer.txt", "newer");
    let (held_name, candidate_name) = if held.as_ref().is_some_and(|held| held.ahead) { (candidate_name, held_name) } else { (held_name, candidate_name) };
    let candidate = state.join("north/runtimes").join(&candidate_name).join("tree/bin/north");
    std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
    north_base::foundation::shim::write_executable(&candidate, script.replace("{NEWER}", &newer));
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
    Promoted { status: output.status.success(), stdout, calls, alarms: std::fs::read_to_string(&alarmed).unwrap_or_default(), candidate: candidate_name }
}

pub(super) fn unfit_signature(log: &Path) -> String {
    format!("failed {}", log.display())
}

#[test]
fn promote_judges_each_recorded_round_against_the_candidate_and_follows_the_replayed_candidate_even_when_current_moves_mid_replay() {
    promote_follows_a_candidate_that_fails_nothing_the_held_runtime_does_not_and_holds_on_a_new_failure();
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
    assert!(stdout.contains("north land promote: refused: round lane-a on ") && stdout.contains("recorded landed but the candidate replayed failed; no held runtime to bisect from; the lander link is unchanged"), "{stdout}");

    let forward = dry_run_while("landed", "ln -sfn runtimes/{NEWER} \"$XDG_STATE_HOME/north/current\";", unfit_signature);
    let stdout = &forward.stdout;
    assert!(forward.status, "{stdout}");
    assert!(stdout.contains(&format!("would point at runtimes/{}", forward.candidate)) && !stdout.contains("refused"), "a current that moved on to a newer runtime mid-replay still gets the replayed candidate: {stdout}");

    let sideways = dry_run_while("landed", "ln -sfn runtimes/elsewhere \"$XDG_STATE_HOME/north/current\";", unfit_signature);
    let stdout = &sideways.stdout;
    assert!(!sideways.status, "{stdout}");
    assert!(stdout.contains(&format!("moved from runtimes/{} to runtimes/elsewhere while the candidate replayed; the lander link is unchanged", sideways.candidate)), "{stdout}");

    let behind = scenario(replay_script("landed", &[]), Some(Held { script: replay_script("landed", &[]), change: Some("Make main green again\n\nMain-red: #671"), held_for: None, ahead: true }), unfit_signature);
    let stdout = &behind.stdout;
    assert!(behind.status, "{stdout}");
    assert!(stdout.contains("the link is not moved back"), "a stale promote never moves the lander link back to an older runtime: {stdout}");
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
    scenario(replay_script("failed", candidate), Some(Held { script: replay_script("failed", held), change, held_for: None, ahead: false }), unfit_signature)
}

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

    let regressed = scenario(replay_script("failed", &[PRE_EXISTING]), Some(Held { script: replay_script("landed", &[]), change: None, held_for: None, ahead: false }), unfit_signature);
    assert!(!regressed.status, "{}", regressed.stdout);
    assert!(regressed.stdout.contains("the held runtime replayed landed"), "{}", regressed.stdout);
}

struct Chain {
    root: tempfile::TempDir,
    checkout: PathBuf,
    state: PathBuf,
    queue: PathBuf,
    held: String,
}

impl Chain {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        std::fs::write(path.join("gitconfig"), "[user]\n\tname = t\n\temail = t@example.com\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n").unwrap();
        let origin = path.join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        git(path, &["clone", "-q", origin.to_str().unwrap(), "main"]);
        let checkout = path.join("main");
        let state = path.join("state");
        let mut chain = Self { checkout, queue: PathBuf::new(), state, held: String::new(), root };
        let base = chain.commit("README", "start");
        let landed = chain.commit("a.txt", "lane a (north#360)");
        chain.queue = queue::directory(&chain.state, &git(&chain.checkout, &["remote", "get-url", "--push", "origin"]));
        let ticket = Ticket { origin: origin.display().to_string(), root: chain.checkout.clone(), branch: "lane-a".to_owned(), head: landed.clone(), decided_on: Some(base), verdict: Some(format!("landed {landed}")), ..Default::default() };
        decided(&chain.queue.join("done"), 1, &ticket);
        chain.runtime(&landed, "echo 'replay: verdict landed'");
        std::os::unix::fs::symlink(format!("runtimes/{landed}"), chain.state.join("north/lander")).unwrap();
        chain.held = landed;
        chain
    }

    fn commit(&self, file: &str, message: &str) -> String {
        let path = self.checkout.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{message}\n")).unwrap();
        git(&self.checkout, &["add", "-A"]);
        git(&self.checkout, &["commit", "-q", "-m", message]);
        git(&self.checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        git(&self.checkout, &["rev-parse", "HEAD"])
    }

    fn runtime(&self, sha: &str, on_replay: &str) {
        let binary = self.state.join(format!("north/runtimes/{sha}/tree/bin/north"));
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        north_base::foundation::shim::write_executable(&binary, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n[ \"$1 $2\" = \"land replay\" ] && {{ {on_replay}; }}\nexit 0\n"));
    }

    fn current(&self, sha: &str) {
        let link = self.state.join("north/current");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(format!("runtimes/{sha}"), link).unwrap();
    }

    fn lander(&self) -> String {
        std::fs::read_link(self.state.join("north/lander")).unwrap().display().to_string()
    }

    fn told(&self) -> String {
        std::fs::read_to_string(self.root.path().join("fleet.calls")).unwrap_or_default()
    }

    fn promote(&self) -> Promoted {
        let fleet = self.root.path().join("fleet");
        north_base::foundation::shim::write_executable(&fleet, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n");
        let arguments = ["promote", "--shadow-minutes", "0", "--repo", self.checkout.to_str().unwrap()].join("\n");
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "serve::lander_process", "--nocapture", "--test-threads", "1"])
            .env(ARGUMENTS, arguments)
            .env("XDG_STATE_HOME", &self.state)
            .env("HOME", self.root.path())
            .env("NORTH_LAND_NORTH", &fleet)
            .env("GIT_CONFIG_GLOBAL", self.root.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Promoted { status: output.status.success(), stdout: String::from_utf8_lossy(&output.stdout).into_owned(), calls: String::new(), alarms: String::new(), candidate: String::new() }
    }

    fn reverts(&self) -> Vec<Ticket> {
        queue::pending(&self.queue).unwrap().into_iter().map(|(_, ticket)| ticket).collect()
    }
}

#[test]
fn promote_follows_a_candidate_whose_commit_declares_the_replay_change_and_logs_the_divergence() {
    let chain = Chain::new();
    let candidate = chain.commit("crates/north-landing/src/land/clippy.rs", "Make lanes need a cached clippy verdict\n\nReplay-change: land.failed lanes need a cached clippy verdict");
    chain.runtime(&candidate, "echo 'replay: verdict failed'");
    chain.current(&candidate);
    let promoted = chain.promote();
    let stdout = &promoted.stdout;
    assert!(promoted.status, "{stdout}");
    assert!(stdout.contains("recorded landed but the candidate replayed failed (the held runtime replayed landed): declared by ") && stdout.contains("(Replay-change: land.failed lanes need a cached clippy verdict)"), "{stdout}");
    assert_eq!(chain.lander(), format!("runtimes/{candidate}"));
    assert!(chain.told().contains("proxy") && chain.told().contains("1 declared replay change(s)"), "{}", chain.told());
    assert!(chain.reverts().is_empty());
}

#[test]
fn a_replay_divergence_that_flips_on_its_rerun_is_a_flaky_check_and_reverts_nothing() {
    let chain = Chain::new();
    let innocent = chain.commit("docs/one.md", "Document one thing");
    let suspect = chain.commit("crates/north-landing/src/land/rule.rs", "Count a dropped connection as no verdict");
    let candidate = chain.commit("docs/two.md", "Document another thing");
    chain.runtime(&innocent, "echo 'replay: verdict landed'");
    chain.runtime(&suspect, "echo x >> \"$0.replays\"; if [ \"$(wc -l < \"$0.replays\")\" -eq 1 ]; then echo 'replay: verdict failed'; else echo 'replay: verdict landed'; fi");
    chain.runtime(&candidate, "echo 'replay: verdict failed'");
    chain.current(&candidate);
    let promoted = chain.promote();
    let stdout = &promoted.stdout;
    assert!(!promoted.status, "{stdout}");
    assert!(stdout.contains(&format!("runtime {} replayed it as recorded on the rerun", &suspect[..9])) && stdout.contains("a flaky check, not a commit's replay change; no commit is reverted"), "{stdout}");
    assert!(chain.reverts().is_empty(), "a check that flips between runs queued a revert: {stdout}");
    assert_eq!(chain.lander(), format!("runtimes/{}", chain.held));
    assert!(chain.told().contains("file the flaky test on its owner"), "{}", chain.told());
}

#[test]
fn promote_bisects_an_undeclared_replay_change_to_its_commit_queues_its_revert_and_follows_once_it_lands() {
    let chain = Chain::new();
    let innocent = chain.commit("docs/one.md", "Document one thing");
    let culprit = chain.commit("crates/north-landing/src/land/rule.rs", "Change a landing rule without saying so");
    let candidate = chain.commit("docs/two.md", "Document another thing");
    chain.runtime(&innocent, "echo 'replay: verdict landed'");
    chain.runtime(&culprit, "echo 'replay: verdict failed'");
    chain.runtime(&candidate, "echo 'replay: verdict failed'");
    chain.current(&candidate);
    let promoted = chain.promote();
    let stdout = &promoted.stdout;
    assert!(!promoted.status, "{stdout}");
    assert!(stdout.contains(&format!("bisected between runtimes {} and {} to {}", &innocent[..9], &culprit[..9], &culprit[..9])), "{stdout}");
    assert_eq!(chain.lander(), format!("runtimes/{}", chain.held));
    let reverts = chain.reverts();
    assert_eq!(reverts.len(), 1, "{stdout}");
    let revert = &reverts[0];
    assert_eq!(revert.branch, format!("sheriff-revert-{}", &culprit[..9]));
    assert!(git(&chain.checkout, &["log", "-1", "--format=%B", &revert.head]).contains(&format!("Sheriff-Reverts: {culprit}")));
    assert!(chain.told().contains("lander link held") && chain.told().contains(&culprit[..9]), "{}", chain.told());

    let again = chain.promote();
    assert!(again.stdout.contains("already queued"), "{}", again.stdout);
    assert_eq!(chain.reverts().len(), 1, "a second promotion queues no second revert: {}", again.stdout);

    git(&chain.checkout, &["push", "-q", "origin", &format!("{}:refs/heads/main", revert.head)]);
    chain.runtime(&revert.head, "echo 'replay: verdict landed'");
    chain.current(&revert.head);
    let next = chain.promote();
    assert!(next.status, "{}", next.stdout);
    assert_eq!(chain.lander(), format!("runtimes/{}", revert.head));
    reaches_the_revert_in_the_same_run_when_a_runtime_between_cannot_replay();
    reverts_nothing_when_a_replay_reaches_no_verdict();
    a_replay_that_reaches_no_verdict_holds_nothing_queues_no_revert_and_the_next_promotion_retries();
}

fn reverts_nothing_when_a_replay_reaches_no_verdict() {
    let unreachable = "echo \"replay: verdict transient main's check environment is broken: mkdir -p on tom@gex45 did not finish within 20s\"";
    let chain = Chain::new();
    let innocent = chain.commit("crates/north-landing/src/land/rule.rs", "Fix an eviction without changing a decision");
    let candidate = chain.commit("docs/two.md", "Document another thing");
    chain.runtime(&innocent, unreachable);
    chain.runtime(&candidate, "echo 'replay: verdict failed'");
    chain.current(&candidate);
    let promoted = chain.promote();
    assert!(!promoted.status, "{}", promoted.stdout);
    assert!(promoted.stdout.contains("which is no verdict") && promoted.stdout.contains("no commit is bisected or reverted"), "{}", promoted.stdout);
    assert!(chain.reverts().is_empty(), "an unreachable check host names no culprit: {}", promoted.stdout);
    let chain = Chain::new();
    let candidate = chain.commit("crates/north-landing/src/land/rule.rs", "Fix an eviction without changing a decision");
    chain.runtime(&candidate, unreachable);
    chain.current(&candidate);
    let promoted = chain.promote();
    assert!(chain.reverts().is_empty(), "a candidate whose own replay reached no verdict is not bisected: {}", promoted.stdout);
}

fn reaches_the_revert_in_the_same_run_when_a_runtime_between_cannot_replay() {
    let chain = Chain::new();
    let broken = chain.commit("docs/one.md", "Document one thing");
    let culprit = chain.commit("crates/north-landing/src/land/rule.rs", "Change a landing rule without saying so");
    let candidate = chain.commit("docs/two.md", "Document another thing");
    chain.runtime(&broken, "exit 1");
    chain.runtime(&culprit, "echo 'replay: verdict failed'");
    chain.runtime(&candidate, "echo 'replay: verdict failed'");
    chain.current(&candidate);
    let promoted = chain.promote();
    let stdout = &promoted.stdout;
    assert!(!promoted.status, "{stdout}");
    assert!(stdout.contains(&format!("runtime {} cannot replay round lane-a", &broken[..9])), "{stdout}");
    assert!(stdout.contains(&format!("to {}; revert {culprit} ", &culprit[..9])), "{stdout}");
    assert_eq!(chain.reverts().len(), 1, "{stdout}");
}

fn a_replay_that_reaches_no_verdict_holds_nothing_queues_no_revert_and_the_next_promotion_retries() {
    let chain = Chain::new();
    let candidate = chain.commit("crates/north-landing/src/land/rule.rs", "Change a landing rule without saying so");
    chain.runtime(&candidate, "echo \"replay: verdict transient main's check environment is broken: mkdir -p on tom@gex45 did not finish within 20s and was stopped\"");
    chain.current(&candidate);
    let unreached = chain.state.join("north/lander-unreached");
    for attempt in 1..=2 {
        let promoted = chain.promote();
        let stdout = &promoted.stdout;
        assert!(!promoted.status, "{stdout}");
        assert!(stdout.contains("but the candidate's replay reached no verdict: transient main's check environment is broken") && stdout.contains("neither held nor bisected"), "{stdout}");
        assert!(!stdout.contains("bisect:"), "a replay with no verdict was bisected: {stdout}");
        assert!(chain.reverts().is_empty(), "a replay with no verdict queued a revert: {stdout}");
        assert!(!chain.state.join("north/lander-held").exists(), "a replay with no verdict held the link: {stdout}");
        assert_eq!(chain.told(), "", "a replay with no verdict alarmed the proxy");
        assert_eq!(chain.lander(), format!("runtimes/{}", chain.held));
        assert!(std::fs::read_to_string(&unreached).unwrap().starts_with(&format!("{candidate}\t{attempt}\t")), "{stdout}");
    }
    chain.runtime(&candidate, "echo 'replay: verdict landed'");
    let promoted = chain.promote();
    assert!(promoted.status, "{}", promoted.stdout);
    assert_eq!(chain.lander(), format!("runtimes/{candidate}"));
    assert!(!unreached.exists(), "a promotion that settled left its no-verdict record");
}
