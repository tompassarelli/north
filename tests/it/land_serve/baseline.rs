use std::fs;
use std::path::PathBuf;

use super::{Fixture, executable, git};

const REAL_CHECKER: &str = r#"#!/usr/bin/env bash
set -u
log=$(mktemp) outcome=$(mktemp) status=0
NORTH_LAND_NORTH="$LAND_REAL_NORTH" "$LAND_REAL_NORTH" land checks --base "$3" --head HEAD --outcome "$outcome" >"$log" 2>&1 || status=$?
cat "$log"
if [ "$status" -eq 0 ]; then printf 'safe-push: verdict green %s\n' "$(git rev-parse HEAD)"
elif [ "$status" -eq 75 ]; then printf 'safe-push: verdict transient %s\n' "$(awk -F'\t' '$1 == "skip" { line = $2 } END { print line }' "$outcome")"
else printf 'safe-push: verdict refused %s\n' "$(grep -m1 'safe-push: land check ' "$log" | grep -v ': land check: ')"
fi
"#;

#[test]
fn serve_hung_check_judged_against_base() {
    let mut fixture = Fixture::new();
    let step = fixture.trace.join("step.sh");
    git(&fixture.repo, &["config", "--file", ".safe-push", "land.check", &format!("* :: bash {}", step.display())]);
    git(&fixture.repo, &["add", ".safe-push"]);
    git(&fixture.repo, &["commit", "-qm", "check every change"]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    executable(&fixture.trace.join("checker"), REAL_CHECKER);
    let serve = |fixture: &Fixture| {
        let output = fixture.command().env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
        eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        output
    };

    let a = fixture.lane("a", "a.txt", "a");
    let first = fixture.enqueue(&a, None);
    fs::write(&step, "exit 0\n").unwrap();
    assert!(serve(&fixture).status.success());
    assert!(fixture.verdict(&first).is_some_and(|verdict| verdict.starts_with("landed ")), "{:?}", fixture.verdict(&first));

    let b = fixture.lane("b", "b.txt", "b");
    let second = fixture.enqueue(&b, None);
    fs::write(&step, "sleep 600\n").unwrap();
    let main = fixture.main();
    assert!(serve(&fixture).status.success());
    let verdict = fixture.verdict(&second).unwrap_or_default();
    assert!(verdict.starts_with("failed ") && verdict.contains(&format!("land check bash {} hung: it ran past its 2 s deadline on this lane, while base tree ", step.display())), "{verdict}");
    assert_eq!(fixture.main(), main, "a hung candidate is not pushed");

    fs::remove_dir_all(fixture.state.join("safe-push/check-timings")).unwrap();
    let c = fixture.lane("c", "c.txt", "c");
    let third = fixture.enqueue(&c, None);
    assert_eq!(serve(&fixture).status.code(), Some(75), "a hang on a base tree never timed is infrastructure");
    assert!(third.exists(), "a hang the base tree was never timed for decided the lane");
    assert_eq!(fixture.main(), main);
}

const RUNNER_CHECKER: &str = r#"#!/usr/bin/env bash
set -u
printf '%s %s %s\n' "$(git rev-parse HEAD)" "$3" "$*" >>"$LAND_TRACE/checks"
[ ! -x "$LAND_TRACE/hook" ] || "$LAND_TRACE/hook"
unset AGENT_CAPACITY_LEASE SAFE_PUSH_LAND_CAPACITY SAFE_PUSH_LAND_OFFLOAD_HOST SAFE_PUSH_LAND_RUNNER SAFE_PUSH_LAND_CHECK_TIMEOUT
export XDG_RUNTIME_DIR="$LAND_TRACE/runtime"
outcome=$(mktemp)
status=0
NORTH_LAND_NORTH="$LAND_TRACE/north-pass" "$LAND_REAL_NORTH" land checks --base "$3" --head "$(git rev-parse HEAD)" --dry 1 --outcome "$outcome" || status=$?
skip=$(awk -F'\t' '$1 == "skip" { found = $2 } END { print found }' "$outcome")
case $status in
  0) echo 'safe-push: verdict green' ;;
  75) echo "safe-push: verdict transient ${skip:-exit 75}" ;;
  *) echo 'safe-push: verdict refused' ;;
esac
[ "$status" -eq 0 ]
"#;

const PASSTHROUGH_NORTH: &str = "#!/usr/bin/env bash\n[ \"$1\" != land-cache ] || exit 1\nexec \"$LAND_REAL_NORTH\" \"$@\"\n";

const PLANTED_CHECK: &str = r#"printf '%s\n' "$(git rev-parse HEAD)" >>"$LAND_TRACE/check-runs"; [ ! -e "$LAND_TRACE/plant" ] || bash "$LAND_TRACE/plant"; if grep -qs broken ./*.txt; then echo "error: a lane wrote broken" >&2; exit 1; fi"#;

impl Fixture {
    fn with_runner_check(&mut self, check: &str) {
        executable(&self.trace.join("checker"), RUNNER_CHECKER);
        executable(&self.trace.join("north-pass"), PASSTHROUGH_NORTH);
        executable(&self.bin.join("supply-alert"), "#!/usr/bin/env bash\nprintf '%s|' \"$@\" >>\"$LAND_TRACE/alerts\"\necho >>\"$LAND_TRACE/alerts\"\n");
        fs::create_dir_all(self.trace.join("runtime")).unwrap();
        git(&self.repo, &["config", "--file", ".safe-push", "--add", "land.check", &format!("*.txt,Cargo.lock :: {check}")]);
        git(&self.repo, &["add", ".safe-push"]);
        git(&self.repo, &["commit", "-qm", "land check"]);
        git(&self.repo, &["push", "-q", "origin", "main"]);
        self.base = git(&self.repo, &["rev-parse", "HEAD"]);
    }

    fn alerts(&self) -> Vec<String> {
        self.read("alerts").lines().map(str::to_owned).collect()
    }

    fn pause(&self) -> PathBuf {
        self.queue().join(north_landing::land::pause::FILE)
    }

    fn retry_now(&self) {
        let mut pause: serde_json::Value = serde_json::from_str(&fs::read_to_string(self.pause()).unwrap()).unwrap();
        pause["retry_at"] = serde_json::json!(0);
        fs::write(self.pause(), pause.to_string()).unwrap();
    }

    fn base_runs(&self) -> usize {
        self.read("check-runs").lines().filter(|line| *line == self.base).count()
    }
}

#[test]
fn serve_pauses_while_main_fails_its_own_check() {
    let mut fixture = Fixture::new();
    fixture.with_runner_check(PLANTED_CHECK);
    let a = fixture.lane("a", "a.txt", "a");
    let b = fixture.lane("b", "b.txt", "b");
    let tickets = [fixture.enqueue(&a, None), fixture.enqueue(&b, None)];
    fs::write(fixture.trace.join("plant"), "echo 'bash: nix: command not found' >&2\nexit 127\n").unwrap();

    assert_eq!(fixture.serve().status.code(), Some(75));
    assert!(tickets.iter().all(|ticket| ticket.exists()), "a broken check environment decided a lane");
    assert_eq!(fixture.main(), fixture.base);
    let alerts = fixture.alerts();
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert!(alerts[0].starts_with("normal|main's check environment is broken: ") && alerts[0].contains(": bash: nix: command not found|"), "{alerts:?}");
    assert_eq!(fixture.base_runs(), 1, "the base tree reran the failed check other than once");
    let checked = fixture.checks().len();

    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("new candidates are paused and the base check is retried in "), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(fixture.checks().len(), checked, "a paused lander checked a new candidate");

    fixture.retry_now();
    assert_eq!(fixture.serve().status.code(), Some(75));
    assert_eq!(fixture.checks().len(), checked + 1, "the lander did not retry once the pause ran out");
    assert_eq!(fixture.base_runs(), 2, "the retry did not rerun the base check");
    assert_eq!(fixture.alerts().len(), 1, "a still-broken environment alerted again");
    assert!(tickets.iter().all(|ticket| ticket.exists()));

    fs::remove_file(fixture.trace.join("plant")).unwrap();
    fixture.retry_now();
    assert!(fixture.serve().status.success());
    assert!(!fixture.pause().exists(), "the lander stayed paused after main passed its check");
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    assert!(tickets.iter().all(|ticket| fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed "))));
    assert_eq!(fixture.alerts().len(), 1);
}

#[test]
fn serve_refuses_a_lane_that_alone_fails_a_check_main_passes() {
    let mut fixture = Fixture::new();
    fixture.with_runner_check(PLANTED_CHECK);
    let a = fixture.lane("a", "a.txt", "broken");
    let ticket = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert!(fixture.verdict(&ticket).is_some_and(|verdict| verdict.starts_with("failed ")), "{:?}", fixture.verdict(&ticket));
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.base_runs(), 1, "the refusal was not judged against the base tree");
    assert!(fixture.alerts().is_empty());
    assert!(!fixture.pause().exists());
}

const RUNTIME_PATH_CHECK: &str = r#"printf '%s\n' "$(git rev-parse HEAD)" >>"$LAND_TRACE/check-runs"; if git diff --quiet "$(cat "$LAND_TRACE/runtime-sha")" "${SAFE_PUSH_LAND_RUNTIME_HEAD:-$SAFE_PUSH_LAND_HEAD}" -- Cargo.lock; then exit 0; fi; echo 'nix: command not found behind the north capacity gate' >&2; exit 127"#;

#[test]
fn serve_judges_a_failure_off_the_runtime_path_against_the_base_on_the_same_path() {
    let mut fixture = Fixture::new();
    fixture.with_runner_check(RUNTIME_PATH_CHECK);
    fs::write(fixture.trace.join("runtime-sha"), &fixture.base).unwrap();
    let a = fixture.lane("a", "Cargo.lock", "lockfile change");
    let ticket = fixture.enqueue(&a, None);
    assert_eq!(fixture.serve().status.code(), Some(75), "a lane forced off the installed runtime was refused for the environment");
    assert!(ticket.exists(), "the off-runtime failure decided the lane");
    assert_eq!(fixture.base_runs(), 1, "the base tree did not rerun the check");
    assert!(fixture.pause().exists());
    assert_eq!(fixture.alerts().len(), 1, "{:?}", fixture.alerts());
}
