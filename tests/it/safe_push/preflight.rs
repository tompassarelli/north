use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use north_core::foundation::shim;

use super::{CLEAN, Fixture, Real, Run, Scratch, git};

#[test]
fn a_code_health_refusal_does_not_block_a_dry_run_and_code_health_never_runs_before_the_landing() {
    let scratch = Scratch::new();
    let fixture = Real::new(&scratch, "code-health");
    let stub = scratch.path().join("code-health-stub");
    let trace = scratch.path().join("code-health-trace");
    shim::write_script(&stub, "printf 'code-health %s\\n' \"$*\" >>\"$CODE_HEALTH_TEST_TRACE\"\nexit \"${CODE_HEALTH_TEST_EXIT:-0}\"\n");
    let observed = Run::new(&scratch, &fixture.repo, &["--dry-run", "--to", "main"])
        .env("SAFE_PUSH_CODE_HEALTH", &stub)
        .env("CODE_HEALTH_TEST_EXIT", "1")
        .env("CODE_HEALTH_TEST_TRACE", &trace)
        .output();
    assert_eq!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains(CLEAN), "{}", observed.output);
    assert!(std::fs::read_to_string(&trace).unwrap_or_default().is_empty(), "code-health ran before the landing");
}

fn land_logs(fixture: &Fixture) -> Vec<String> {
    let mut logs: Vec<String> = fs::read_dir(fixture.state.join("safe-push"))
        .map(|entries| entries.map(|entry| entry.unwrap().path().to_string_lossy().into_owned()).filter(|path| path.contains("/land-check-") && path.ends_with(".log")).collect())
        .unwrap_or_default();
    logs.sort();
    logs
}

fn backdate(path: &Path, days: u64) {
    fs::File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(days * 86_400)).unwrap();
}

#[test]
fn a_fresh_north_worktree_installs_agent_machinery_before_its_land_check() {
    let fixture = Fixture::new();
    fixture.origin_named_north();
    fs::create_dir_all(fixture.repo.join("agent-machinery")).unwrap();
    fs::write(fixture.repo.join("agent-machinery/package.json"), "{}\n").unwrap();
    fs::write(fixture.repo.join("agent-machinery/bun.lock"), "{\n  \"lockfileVersion\": 1,\n  \"workspaces\": {\"\": {\"name\": \"agent-machinery\"}},\n  \"packages\": {}\n}\n").unwrap();
    fixture.git(&["add", "agent-machinery"]);
    fixture.land_checks(&["agent-machinery/ :: test -d agent-machinery/node_modules"], "agent-machinery project");
    shim::write_script(&fixture.bin.join("bun"), "[ \"$*\" = \"install --frozen-lockfile\" ] || exit 2\nmkdir -p node_modules\n");
    let installing = format!("safe-push: no node_modules — bun install --frozen-lockfile in {}/agent-machinery", fixture.repo.display());
    fixture.run(&["--dry-run", "--to", "main"]).expect(true, &[&installing, CLEAN]);
}

#[test]
fn a_landing_that_undoes_recent_main_is_refused_with_its_land_checks_still_run() {
    let fixture = Fixture::new();
    fixture.land_checks(&["content.txt :: printf 'ran\\n' >> check-runs"], "checks on content.txt");
    let mover = fixture.root.join("mover");
    git(&fixture.root, &["clone", "-q", &fixture.remote.to_string_lossy(), "mover"]);
    git(&mover, &["config", "user.name", "safe-push-test"]);
    git(&mover, &["config", "user.email", "safe-push-test@example.invalid"]);
    fs::write(mover.join("content.txt"), "moved\n").unwrap();
    git(&mover, &["commit", "-qam", "main moves content.txt"]);
    git(&mover, &["push", "-q", "origin", "main"]);
    fixture.git(&["fetch", "-q", "origin"]);
    fixture.git(&["reset", "-q", "--soft", "origin/main"]);
    fixture.git(&["commit", "-qm", "squash without a rebase"]);
    let before = fixture.remote_state();
    fixture.run(&["--dry-run", "--to", "main"]).expect(false, &["safe-push: ✗ this landing undoes recent commits on main", "stale revert of newer main"]);
    assert!(fixture.repo.join("check-runs").exists(), "a stale-revert refusal skipped the land checks instead of reporting them too");
    assert_eq!(fixture.remote_state(), before, "a stale-revert refusal published");
}

#[test]
fn a_land_check_failure_keeps_its_output_in_a_state_log_a_pass_keeps_none_and_only_committed_matching_checks_run() {
    let fixture = Fixture::new();
    let failure_log = fails_into_a_state_log_with_events_and_north_watch(&fixture);
    passes_with_visible_output_and_no_log(&fixture, &failure_log);
    leaves_a_background_process_no_private_descriptor(&fixture);
    runs_only_committed_matching_checks(&fixture);
}

fn fails_into_a_state_log_with_events_and_north_watch(fixture: &Fixture) -> String {
    let command = r#"printf "ran\n" >> check-runs; printf "PASS before\n"; printf "FAIL named-land-case\n" >&2; printf "PASS after\n"; exit 23"#;
    fixture.land_checks(&[&format!("file.txt,other.txt :: {command}"), "unmatched/ :: exit 71"], "affected checks");
    fixture.commit("file.txt", "changed\n", "changed input");
    let before = fixture.remote_state();
    let watch_calls = fixture.root.join("watch-calls");
    let watch = fixture.root.join("watch-stub");
    shim::write_script(&watch, &format!("printf '%s\\n' \"$*\" >>'{}'\n", watch_calls.display()));
    let ran = fixture.run_with(&["--dry-run", "--to", "main"], &[("SAFE_PUSH_WATCH", &watch)]);
    assert!(!ran.succeeded(), "{}", ran.output);
    let log = ran.output.split_once("exited 23; log: ").and_then(|(_, rest)| rest.split_once("; NOT pushed.")).map(|(log, _)| log.to_owned()).unwrap_or_else(|| panic!("{}", ran.output));
    ran.expect(false, &[&format!("safe-push: land check {command} exited 23; log: {log}; NOT pushed.")]);
    assert_eq!(land_logs(fixture), [log.as_str()], "the failure did not name its one absolute state log");
    let mut kept: Vec<String> = fs::read_to_string(&log).unwrap().lines().map(str::to_owned).collect();
    kept.sort();
    assert_eq!(kept, ["FAIL named-land-case", "PASS after", "PASS before"], "the failure log changed the check's output");
    assert_eq!(fs::read_to_string(fixture.repo.join("check-runs")).unwrap(), "ran\n", "the matching check did not run exactly once");
    assert!(!fixture.traced().contains("<land-clean>"), "a failed land check released the lane build output its retry reuses");
    assert_eq!(fixture.remote_state(), before, "a failing land check published");
    let events = fs::read_to_string(fixture.state.join("safe-push/events.jsonl")).unwrap();
    for wanted in [r#""stage":"land check","status":"failed","detail":""#, &format!("log: {log}\""), r#""stage":"exit","status":"refused""#] {
        assert!(events.contains(wanted), "no event {wanted}: {events}");
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read_to_string(&watch_calls).is_ok_and(|calls| calls.lines().any(|call| call == "watch --once")) {
        assert!(Instant::now() < deadline, "a failed landing did not start north watch at once");
        std::thread::sleep(Duration::from_millis(20));
    }
    log
}

fn passes_with_visible_output_and_no_log(fixture: &Fixture, failure_log: &str) {
    let command = r#"printf "PASS stdout\n"; printf "PASS stderr\n" >&2"#;
    fixture.land_checks(&[&format!("file.txt :: {command}")], "successful check");
    let logs = fixture.state.join("safe-push");
    let (stale, unrelated) = (logs.join("land-check-old.log"), logs.join("unrelated.log"));
    for path in [&stale, &unrelated] {
        fs::write(path, "old").unwrap();
        backdate(path, 16);
    }
    fixture.clear_trace();
    let ran = fixture.run(&["--dry-run", "--to", "main"]);
    ran.expect(true, &[&format!("safe-push: land check: {command}"), "PASS stdout", "PASS stderr", CLEAN]);
    assert!(!ran.output.contains("log:"), "a successful check added visible logging output: {}", ran.output);
    assert!(fixture.traced().lines().any(|line| line == format!("north <land-clean> <{}>", fixture.repo.display())), "a passing land check left the lane build output");
    for args in [&["--dry-run", "--to", "main", "--keep-lane"][..], &["--dry-run", "--to", "other"]] {
        fixture.clear_trace();
        fixture.run(args).expect(true, &[CLEAN]);
        assert!(!fixture.traced().contains("<land-clean>"), "{args:?} released the lane build output");
    }
    assert_eq!(land_logs(fixture), [failure_log], "a successful check kept a log or removed the failure log");
    assert!(!stale.exists(), "an old failure log was not pruned");
    assert!(unrelated.exists(), "pruning removed an unrelated log");
}

fn descriptors(pid: &str) -> Vec<(String, String)> {
    fs::read_dir(format!("/proc/{pid}/fd"))
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            Some((entry.file_name().to_string_lossy().into_owned(), fs::read_link(entry.path()).ok()?.to_string_lossy().into_owned()))
        })
        .collect()
}

fn leaves_a_background_process_no_private_descriptor(fixture: &Fixture) {
    fixture.land_checks(&[r#"file.txt :: sleep 30 >/dev/null 2>&1 & printf "%s\n" "$!" > sleeper.pid"#], "background check");
    let before = fixture.remote_state();
    let inherited: Vec<String> = descriptors("self").into_iter().map(|(_, target)| target).collect();
    let ran = fixture.run(&["--dry-run", "--to", "main"]);
    let sleeper = fs::read_to_string(fixture.repo.join("sleeper.pid")).unwrap_or_else(|_| panic!("the background check started no sleeper: {}", ran.output)).trim().to_owned();
    let leaked: Vec<String> = descriptors(&sleeper).into_iter().filter(|(fd, target)| fd.parse::<u32>().is_ok_and(|fd| fd > 2) && !inherited.contains(target)).map(|(fd, target)| format!("{fd}->{target}")).collect();
    let _ = std::process::Command::new("kill").arg(&sleeper).status();
    assert!(leaked.is_empty(), "the background check inherited private descriptors: {leaked:?}");
    ran.expect(true, &[CLEAN]);
    assert_eq!(fixture.remote_state(), before, "a background dry run published");
}

fn runs_only_committed_matching_checks(fixture: &Fixture) {
    fixture.land_checks(&["unmatched/ :: exit 71"], "unmatched check");
    fixture.git(&["config", "--file", ".safe-push", "--add", "land.check", "file.txt :: touch uncommitted-ran"]);
    fixture.clear_trace();
    fixture.run(&["--dry-run", "--to", "main"]).expect(true, &[CLEAN]);
    assert!(!fixture.repo.join("uncommitted-ran").exists(), "an uncommitted land check ran");
    assert_eq!(fs::read_to_string(fixture.repo.join("check-runs")).unwrap(), "ran\n", "an unmatched or uncommitted check ran");
    assert!(!fixture.traced().contains("<land-clean>"), "a push that ran no land check released the lane build output");
    fixture.git(&["checkout", "--", ".safe-push"]);
    fixture.git(&["rm", "-q", ".safe-push"]);
    fixture.git(&["commit", "-qm", "no land.check"]);
    fixture.run(&["--dry-run", "--to", "main"]).expect(true, &[CLEAN]);
}
