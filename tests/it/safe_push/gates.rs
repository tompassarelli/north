use super::{CLEAN, Fixture, Landing, Run, Scratch, git, write};
use std::path::{Path, PathBuf};

struct Case {
    landing: Landing,
    lane: PathBuf,
}

fn case(scratch: &Scratch, name: &str, settings: &[(&str, &str)], checks: &[&str]) -> Case {
    let landing = Landing::new(scratch, name);
    let lane = landing.lane();
    let local = format!("touch '{}'", landing.root.join("local-ran").display());
    git(&lane, &["config", "--file", ".safe-push", "--add", "land.local-only", &format!("needs this host :: {local}")]);
    let mut checks = checks.to_vec();
    checks.push(&local);
    landing.configure(&lane, settings, &checks, &format!("runner case {name}"));
    Case { landing, lane }
}

fn dry_run(scratch: &Scratch, case: &Case, environment: &[(&str, &Path)], values: &[(&str, &str)]) -> super::Observed {
    let mut run = Run::new(scratch, &case.lane, &["--dry-run", "--to", "main"]);
    for (name, path) in environment {
        run = run.env(name, path);
    }
    for (name, value) in values {
        run = run.env(name, value);
    }
    run.output()
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn land_runner_build_host_resolves_to_a_host_with_room_and_runs_every_portable_check_there_at_once_and_the_local_only_check_here() {
    let scratch = Scratch::new();
    let meet = scratch.path().join("meet.sh");
    write(&meet, "touch \"$PWD/../started-$1\"; for i in $(seq 100); do [ -e \"$PWD/../started-$2\" ] && exit 0; sleep 0.1; done; exit 9\n");
    let one = format!("sh '{}' one two", meet.display());
    let two = format!("sh '{}' two one", meet.display());
    let case = case(&scratch, "runner-overlap", &[("land.runner", "build-host")], &[&one, &two]);
    let offloaded = case.landing.root.join("offloaded");
    let observed = dry_run(&scratch, &case, &[("SAFE_PUSH_TEST_OFFLOAD_TRACE", &offloaded)], &[("SAFE_PUSH_TEST_BUILD_HOST", "builder")]);
    assert_eq!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains(CLEAN), "{}", observed.output);
    let sent = lines(&offloaded);
    assert_eq!(sent.len(), 2, "land.runner did not send both portable checks to the build host: {sent:?}");
    assert!(sent.iter().all(|line| line.starts_with("builder\t")), "land.runner did not resolve build-host to the build host with room: {sent:?}");
    assert!(case.landing.root.join("local-ran").exists(), "land.runner never ran the local-only check here");
    assert!(!sent.iter().any(|line| line.contains("local-ran")), "land.runner sent a local-only check to the build host: {sent:?}");
    assert!(observed.output.contains("safe-push: stage land checks:"), "{}", observed.output);
    let stages = std::fs::read_to_string(scratch.state().join("safe-push/stages.tsv")).unwrap();
    let stage_ms: u64 = stages.lines().filter_map(|line| line.split('\t').collect::<Vec<_>>().get(3..5).filter(|fields| fields[0] == "land checks").map(|fields| fields[1].parse().unwrap())).next_back().unwrap_or(99999);
    assert!(stage_ms < 15000, "land.runner checks did not overlap: land-check stage took {stage_ms} ms");
}

#[test]
fn an_actions_dispatch_exiting_with_the_infrastructure_code_reruns_its_checks_on_the_first_build_host_with_room_and_takes_that_verdict() {
    let scratch = Scratch::new();
    let case = case(&scratch, "actions-infra-build-host", &[("land.workflow", "land-check.yml")], &["true"]);
    let offloaded = case.landing.root.join("offloaded");
    let observed = dry_run(&scratch, &case, &[("SAFE_PUSH_TEST_OFFLOAD_TRACE", &offloaded)], &[("SAFE_PUSH_TEST_ACTIONS_STATUS", "75"), ("SAFE_PUSH_TEST_BUILD_HOST", "builder")]);
    assert_eq!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains(CLEAN), "{}", observed.output);
    assert_eq!(lines(&offloaded), ["builder\ttrue"], "Actions infrastructure did not re-run the check on the build host");
    assert!(observed.output.contains("GitHub Actions is unavailable (infrastructure, not a check failure)"), "{}", observed.output);
}

#[test]
fn with_actions_down_and_no_build_host_with_room_the_checks_run_here() {
    let scratch = Scratch::new();
    let case = case(&scratch, "actions-infra-here", &[("land.workflow", "land-check.yml")], &["touch \"$PWD/../portable-here\""]);
    let offloaded = case.landing.root.join("offloaded");
    let observed = dry_run(&scratch, &case, &[("SAFE_PUSH_TEST_OFFLOAD_TRACE", &offloaded)], &[("SAFE_PUSH_TEST_ACTIONS_STATUS", "75")]);
    assert_eq!(observed.status, Some(0), "{}", observed.output);
    assert!(observed.output.contains(CLEAN), "{}", observed.output);
    assert!(lines(&offloaded).is_empty() && case.landing.root.join("portable-here").exists(), "with no build host room the check did not run here: {}", observed.output);
    assert!(observed.output.contains("no build host has room"), "{}", observed.output);
}

#[test]
fn a_real_check_failure_on_actions_or_a_build_host_refuses_the_landing_and_is_never_retried_elsewhere() {
    let scratch = Scratch::new();
    let actions = case(&scratch, "actions-check-failure", &[("land.workflow", "land-check.yml")], &["true"]);
    let offloaded = actions.landing.root.join("offloaded");
    let failed = dry_run(&scratch, &actions, &[("SAFE_PUSH_TEST_OFFLOAD_TRACE", &offloaded)], &[("SAFE_PUSH_TEST_ACTIONS_STATUS", "1"), ("SAFE_PUSH_TEST_BUILD_HOST", "builder")]);
    assert!(failed.status != Some(0) && !failed.output.contains(CLEAN), "a failed Actions check landed: {}", failed.output);
    assert!(lines(&offloaded).is_empty(), "a failed Actions check was retried on the build host");

    let runner = case(&scratch, "runner-check-failure", &[("land.runner", "build-host")], &["printf 'ran\\n' >>\"$PWD/../runs\"; exit 7"]);
    let offloaded = runner.landing.root.join("offloaded");
    let failed = dry_run(&scratch, &runner, &[("SAFE_PUSH_TEST_OFFLOAD_TRACE", &offloaded)], &[("SAFE_PUSH_TEST_BUILD_HOST", "builder")]);
    assert!(failed.status != Some(0) && !failed.output.contains(CLEAN), "a failed build-host check landed: {}", failed.output);
    assert_eq!(lines(&runner.landing.root.join("runs")).len(), 1, "a failed build-host check was retried");
    assert!(failed.output.contains("exited 7"), "a failed build-host check lost its report: {}", failed.output);
}

fn gate_call(fixture: &Fixture, base: &str, head: &str) -> String {
    format!("north <language-gate> <--repo> <{}> <--base> <{base}> <--head> <{head}>", fixture.repo.display())
}

#[test]
fn a_language_gate_refusal_blocks_the_landing_and_names_the_gate() {
    let fixture = Fixture::new();
    let before = fixture.remote_state();
    let (base, head) = (git(&fixture.remote, &["rev-parse", "main"]), fixture.head());
    fixture.deny_language("new.py: Python files are forbidden");
    let ran = fixture.run(&["--dry-run", "--to", "main"]);
    ran.expect(false, &["new.py: Python files are forbidden"]);
    assert_eq!(fixture.remote_state(), before, "a language refusal published");
    let summary = ran.output.lines().find_map(|line| line.split_once("blocking check(s) failed: ")).map(|(_, failed)| failed.to_owned());
    assert!(summary.is_some_and(|failed| failed.split("; ").any(|gate| gate.starts_with("language gate"))), "{}", ran.output);
    assert!(fixture.traced().lines().any(|line| line == gate_call(&fixture, &base, &head)), "{}", fixture.traced());
}

#[test]
fn an_initial_push_hands_the_language_gate_the_empty_tree_as_base() {
    let fixture = Fixture::new();
    for line in fixture.remote_state().lines() {
        let reference = line.split_once(' ').unwrap().0;
        git(&fixture.remote, &["update-ref", "-d", reference]);
    }
    let (empty_tree, head) = (fixture.git(&["hash-object", "-t", "tree", "/dev/null"]), fixture.head());
    fixture.deny_language("initial push denied by language gate");
    fixture.run(&["--dry-run", "--to", "main"]).expect(false, &["initial push denied by language gate"]);
    assert!(fixture.traced().lines().any(|line| line == gate_call(&fixture, &empty_tree, &head)), "{}", fixture.traced());
    assert_eq!(fixture.remote_state(), "", "an initial language refusal published");
}

#[test]
fn a_failing_land_check_and_a_secret_are_refused_once_naming_both() {
    let fixture = Fixture::new();
    fixture.land_checks(&["file.txt :: printf \"FAIL boundary\\n\" >&2; exit 9"], "a test that fails");
    fixture.commit("file.txt", "changed\n", "change under test beside a secret fixture");
    let before = fixture.remote_state();
    fixture.leak_secret();
    let ran = fixture.run(&["--dry-run", "--to", "main"]);
    ran.expect(false, &["safe-push: ✗ 2 blocking check(s) failed: land checks; secret scan; NOT pushed.", "safe-push: ✗ SECRET DETECTED"]);
    assert_eq!(ran.output.lines().filter(|line| line.contains("✗ ") && line.contains(" blocking check(s) failed")).count(), 1, "the refusal was not reported once: {}", ran.output);
    assert_eq!(fixture.remote_state(), before, "a refused landing published");
}

#[test]
fn a_commit_message_naming_a_native_item_refuses_before_landing() {
    let fixture = Fixture::new();
    fixture.git(&["commit", "-q", "--allow-empty", "-m", "query scroll target for ops#17"]);
    fixture.run(&["--dry-run"]).expect(false, &["names a native NorthThread item"]);
}

#[test]
fn a_north_skill_change_lands_with_no_reviewed_trailer() {
    let fixture = Fixture::new();
    fixture.origin_named_north();
    fixture.commit("harness/skills/example/SKILL.md", "changed\n", "change a skill");
    fixture.run(&["--dry-run", "--to", "main"]).expect(true, &[CLEAN]);
}
