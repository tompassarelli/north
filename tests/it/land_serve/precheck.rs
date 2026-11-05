use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use north_landing::land::queue::{self, Ticket};

use super::{Fixture, executable, git};

const ALONE: &str = r#"#!/usr/bin/env bash
lane=$(command git log -1 --format=%s)
stamp() { date +%s.%N >"$LAND_TRACE/.$1" && mv "$LAND_TRACE/.$1" "$LAND_TRACE/$1"; }
stamp "start-${lane##* }-$1"
! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >"$LAND_TRACE/verdict-$1"
sleep 2.5
stamp "end-${lane##* }-$1"
"#;

fn epoch() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

fn starts(fixture: &Fixture, lane: &str) -> Vec<f64> {
    stamps(fixture, "start", lane)
}

fn stamps(fixture: &Fixture, kind: &str, lane: &str) -> Vec<f64> {
    let prefix = format!("{kind}-{lane}-");
    let mut found: Vec<f64> = fs::read_dir(&fixture.trace).unwrap().filter_map(Result::ok).filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix)).map(|entry| fs::read_to_string(entry.path()).unwrap().trim().parse().unwrap()).collect();
    found.sort_by(f64::total_cmp);
    found
}

#[test]
fn precheck_checks_each_lane_alone_as_it_arrives() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "3"]);
    let lanes = [("a", "a.txt", "a"), ("b", "app.txt", "ALPHA\nbeta\ngamma\n"), ("c", "fail.txt", "c"), ("d", "app.txt", "alpha\nbeta\nGAMMA\n"), ("e", "e.txt", "e"), ("f", "f.txt", "f")];
    let roots: Vec<PathBuf> = lanes.iter().map(|(name, file, text)| fixture.lane(name, file, text)).collect();
    let tickets: Vec<(PathBuf, Ticket)> = roots.iter().map(|root| fixture.ticket(root, None)).collect();
    executable(&fixture.trace.join("hook"), ALONE);
    let mut enqueued = vec![epoch()];
    queue::write(&tickets[0].0, &tickets[0].1).unwrap();
    let first = Instant::now();
    let child = fixture.command().stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    for (index, (path, ticket)) in tickets.iter().enumerate().skip(1) {
        std::thread::sleep((first + Duration::from_secs(index as u64)).saturating_duration_since(Instant::now()));
        enqueued.push(epoch());
        queue::write(path, ticket).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    eprintln!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let delays: Vec<f64> = lanes.iter().zip(&enqueued).map(|((name, _, _), queued)| starts(&fixture, name)[0] - queued).collect();
    eprintln!("enqueue to check start, seconds: {delays:?}");
    for pair in lanes.windows(2) {
        let (previous, name) = (pair[0].0, pair[1].0);
        let (start, previous_end) = (starts(&fixture, name)[0], stamps(&fixture, "end", previous)[0]);
        assert!(start < previous_end, "lane {name}'s check started {:.3} s after lane {previous}'s check ended; it waited for the previous candidate: {delays:?}", start - previous_end);
    }
    let refused = fixture.verdict(&tickets[2].0).unwrap();
    assert!(refused.starts_with("failed ") && refused.ends_with("-precheck.log"), "{refused}");
    assert_eq!(fixture.fleet().iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-c ")).count(), 1);
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from e", "change from f", "change from d"], "green lanes land as their checks finish: e and f land while d is rechecked in full");
    for (index, (path, _)) in tickets.iter().enumerate().filter(|(index, _)| *index != 2) {
        assert!(fixture.verdict(path).is_some_and(|verdict| verdict.starts_with("landed ")), "lane {} did not land", lanes[index].0);
    }
    assert_eq!(fixture.verdict(&tickets[3].0), Some(format!("landed {}", fixture.main())));
    for (name, _, _) in lanes.iter().filter(|(name, _, _)| *name != "d") {
        assert_eq!(starts(&fixture, name).len(), 1, "lane {name} is checked once");
    }
    assert_eq!(starts(&fixture, "d").len(), 2, "lane d, whose app.txt lane b changed after d's check started, is rechecked once");
    let after_b = git(&fixture.origin, &["log", "--format=%H", "--grep=change from b", "main"]);
    let checks = fixture.checks();
    assert_eq!(checks.len(), 7, "{checks:?}");
    assert_eq!(stdout.matches("rechecking it on main@").count(), 1, "{stdout}");
    assert!(stdout.contains(&format!("lane-d changes app.txt that main changed since its check started; rechecking it on main@{after_b}")), "d is rechecked on the main lane b landed: {stdout}");
    assert!(checks.iter().any(|(_, base)| *base == after_b), "{checks:?}");
    assert_eq!(stdout.matches("running the recheck lines on the tree it would land").count(), 3, "b, e and f, whose files main did not touch, take the light recheck on the main they land on: {stdout}");
    assert_eq!(fixture.read("rechecks").lines().count(), 3, "the light rechecks run with NORTH_LAND_RECHECK set and skip the full check");
    assert_eq!(fixture.read("pushes").lines().count(), 5);
}

#[test]
fn precheck_falls_back_to_one_batch_when_the_build_slots_cannot_be_prepared() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "2"]);
    let tickets: Vec<PathBuf> = ["a", "b"]
        .iter()
        .map(|name| {
            let lane = fixture.lane(name, &format!("{name}.txt"), name);
            fixture.enqueue(&lane, None)
        })
        .collect();
    fs::write(fixture.root.join("proj/worktrees/lander-precheck-0"), "not a worktree").unwrap();
    let output = fixture.serve();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("the 2 build slots cannot be prepared"));
    assert_eq!(fixture.checks().len(), 1, "both lanes are checked as one candidate");
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    let main = fixture.main();
    for ticket in &tickets {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
}

#[test]
fn a_runtime_change_lets_the_running_precheck_finish_and_land_before_the_lander_exits() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "1"]);
    let link = fixture.root.join("runtime-current");
    std::os::unix::fs::symlink("gen-1", &link).unwrap();
    executable(&fixture.trace.join("hook"), ALONE);
    let a = fixture.lane("a", "a.txt", "a");
    let b = fixture.lane("b", "b.txt", "b");
    let (path_a, ticket_a) = fixture.ticket(&a, None);
    let (path_b, ticket_b) = fixture.ticket(&b, None);
    queue::write(&path_a, &ticket_a).unwrap();
    queue::write(&path_b, &ticket_b).unwrap();
    let command = fixture.command_with(["--repo", fixture.repo.to_str().unwrap()]);
    let arguments: Vec<String> = command.get_args().map(|argument| argument.to_string_lossy().into_owned()).filter(|argument| argument != "--once").collect();
    let mut serve = Command::new(env!("CARGO_BIN_EXE_north"));
    serve.args(&arguments).envs(command.get_envs().filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))).env("NORTH_RUNTIME_LINK", &link);
    let mut child = serve.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while starts(&fixture, "a").is_empty() {
        assert!(Instant::now() < deadline, "lane a's precheck never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("gen-2", &link).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the lander did not exit after the runtime changed");
        std::thread::sleep(Duration::from_millis(100));
    };
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    eprintln!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(status.code(), Some(0));
    assert!(!stdout.contains("the check of lane-a stopped"), "the runtime change killed lane a's running precheck: {stdout}");
    assert!(stdout.contains("finishing 1 lane check(s) already running before exiting"), "{stdout}");
    assert_eq!(fixture.verdict(&path_a), Some(format!("landed {}", fixture.main())), "lane a's finished precheck lands before the lander exits");
    assert!(starts(&fixture, "b").is_empty(), "no precheck starts once the runtime changed");
    assert!(path_b.exists(), "lane b stays queued for the restarted lander");
}

const OFFLOAD_NORTH: &str = r#"#!/usr/bin/env bash
stamp() { date +%s.%N >"$LAND_TRACE/.$1" && mv "$LAND_TRACE/.$1" "$LAND_TRACE/$1"; }
if [ "$1 $2 $3" = "capacity offload choose" ]; then
  echo stub-a
  exit 0
fi
if [ "$1 $2 $3" = "capacity offload run" ]; then
  lane=$(command git log -1 --format=%s); lane=${lane##* }
  case $lane in a) sleep 8 ;; b) sleep 1 ;; *) sleep 2 ;; esac
  stamp "end-$lane"
fi
if [ "$1 $2" = "fleet send" ] && [ "${6%% *}" = landed ]; then
  stamp "landed-${5#session-lane-}"
fi
exit 0
"#;

const OFFLOAD_CHECK: &str = r#"#!/usr/bin/env bash
"$NORTH_LAND_NORTH" capacity offload run --host "$SAFE_PUSH_LAND_RUNNER" --base HEAD~1 --head HEAD --timeout 60 -- 'cargo test'
"#;

#[test]
fn an_offloaded_green_verdict_lands_while_an_earlier_lane_still_checks() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "3"]);
    git(&fixture.repo, &["config", "land.runner", "build-host"]);
    executable(&fixture.trace.join("north-hook"), OFFLOAD_NORTH);
    executable(&fixture.trace.join("hook"), OFFLOAD_CHECK);
    let tickets: Vec<PathBuf> = ["a", "b", "c"]
        .iter()
        .map(|name| {
            let lane = fixture.lane(name, &format!("{name}.txt"), name);
            fixture.enqueue(&lane, None)
        })
        .collect();
    let output = fixture.serve();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{stdout}");
    let stamp = |name: &str| -> f64 { fixture.read(name).trim().parse().unwrap_or_else(|_| panic!("{name} holds no time: {stdout}")) };
    for name in ["b", "c"] {
        assert!(stamp(&format!("landed-{name}")) < stamp("end-a"), "lane {name}'s green verdict waited for lane a, queued ahead of it, to finish its check: {stdout}");
    }
    let waited = stamp("landed-b") - stamp("end-b");
    assert!(waited < 1.5, "lane b landed {waited:.3} s after its offloaded check ended: {stdout}");
    assert!(stdout.contains("main moved to main@") && stdout.contains("since lane-c was checked"), "lane c, whose check ended after lane b landed, is rechecked on the new main: {stdout}");
    assert_eq!(fixture.subjects(), ["change from b", "change from c", "change from a"], "green lanes land as their checks finish");
    for ticket in &tickets {
        assert!(fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed ")), "{} did not land: {stdout}", ticket.display());
    }
}
