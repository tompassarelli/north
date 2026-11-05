use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use north_landing::land::queue::{self, Ticket};

use super::{Fixture, executable, git, json_names, pushed_trees_were_checked, stamp};

const AWAIT: &str = "await() { for ((i = 0; i < 400; i++)); do [ -e \"$LAND_TRACE/$1\" ] && return 0; sleep 0.05; done; return 1; }\n";
const LANDED: &str = "main() { command git -C \"$LAND_ORIGIN\" rev-parse main; }\nlanded() { for ((i = 0; i < 400; i++)); do [ \"$(main)\" != \"$1\" ] && return 0; sleep 0.05; done; return 1; }\n";

fn speculate_serve(fixture: &mut Fixture, later: &[PathBuf]) -> Output {
    let tickets: Vec<(PathBuf, Ticket)> = later.iter().map(|lane| fixture.ticket(lane, None)).collect();
    let started = std::time::Instant::now();
    let child = fixture.command().stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    while fixture.checks().is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(30), "the first batch's check never started");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    for (path, ticket) in &tickets {
        queue::write(path, ticket).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    output
}

#[test]
fn speculate_two_batches() {
    let mut fixture = Fixture::new();
    let first: Vec<PathBuf> = ["a", "b"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    let second: Vec<PathBuf> = ["c", "d"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    for lane in &first {
        fixture.enqueue(lane, None);
    }
    executable(&fixture.trace.join("hook"), &format!("#!/usr/bin/env bash\n{AWAIT}{LANDED}case $1 in\n1) await check-2 && touch \"$LAND_TRACE/overlapped\" ;;\n2) start=$(main); touch \"$LAND_TRACE/check-2\"; landed \"$start\" && touch \"$LAND_TRACE/landed-during-check-2\" ;;\nesac\n"));
    let output = speculate_serve(&mut fixture, &second);
    assert!(output.status.success());
    assert!(fixture.trace.join("overlapped").exists(), "the second batch's check starts while the first batch's check runs");
    assert!(fixture.trace.join("landed-during-check-2").exists(), "the first batch lands while the second batch's check runs");
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c", "change from d"]);
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "each batch is checked once");
    assert_eq!(checks[0].1, fixture.base);
    assert_eq!(checks[1].1, checks[0].0, "the second batch is checked on the first batch's candidate");
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.read("pushes").lines().count(), 2);
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_discard_on_red() {
    let mut fixture = Fixture::new();
    let bad = fixture.lane("a", "fail.txt", "a");
    let good = fixture.lane("b", "b.txt", "b");
    let culprit = fixture.enqueue(&bad, None);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n{AWAIT}case $1 in 1) await check-2 ;; 2) touch \"$LAND_TRACE/check-2\"; sleep 30; touch \"$LAND_TRACE/check-2-finished\" ;; esac\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n"),
    );
    let output = speculate_serve(&mut fixture, std::slice::from_ref(&good));
    assert!(output.status.success());
    assert!(!fixture.trace.join("check-2-finished").exists(), "the speculative check of a red candidate is stopped, not waited for");
    let checks = fixture.checks();
    assert!(String::from_utf8_lossy(&output.stdout).lines().any(|line| line == format!("north land: speculative candidate {} on {} discarded; its lanes stay queued", checks[1].0, checks[0].0)), "the lander logs the stopped speculative check");
    assert_eq!(checks.len(), 3, "the red candidate, its discarded successor, then the successor rebuilt: {checks:?}");
    assert_eq!(checks[1].1, checks[0].0, "the successor was speculated on the red candidate");
    assert_eq!(checks[2].1, fixture.base, "the successor is rebuilt on main without the culprit");
    assert!(fixture.verdict(&culprit).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from b"]);
    assert_eq!(fixture.main(), checks[2].0);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"));
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_red_after_green() {
    let mut fixture = Fixture::new();
    let good = fixture.lane("a", "a.txt", "a");
    let bad = fixture.lane("b", "fail.txt", "b");
    fixture.enqueue(&good, None);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n{AWAIT}case $1 in 1) await check-2 ;; 2) touch \"$LAND_TRACE/check-2\" ;; esac\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n"),
    );
    let output = speculate_serve(&mut fixture, std::slice::from_ref(&bad));
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "the green candidate, the red one speculated on it, then the red lane rebuilt on main: {checks:?}");
    assert_eq!(checks[0].1, fixture.base);
    assert_eq!(checks[1].1, checks[0].0, "the red candidate was speculated on the green one");
    assert_eq!(checks[2].1, checks[0].0, "the red lane is rebuilt on the main the green candidate landed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.lines().any(|line| line.starts_with(&format!("north land: speculative candidate {} was not green (", checks[1].0))), "the red speculative candidate is discarded: {stdout}");
    let culprit = json_names(&fixture.queue().join("done")).into_iter().find(|name| name.ends_with("-lane-b.json")).unwrap();
    assert!(fixture.verdict(&fixture.queue().join(culprit)).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from a"]);
    assert_eq!(fixture.main(), checks[0].0, "main stays at the green candidate");
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"));
    assert_eq!(fixture.read("pushes").lines().count(), 1, "only the green candidate is pushed");
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_real_gates() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    let b = fixture.lane("b", "b.txt", "b");
    fixture.enqueue(&a, None);
    executable(
        &fixture.trace.join("hook"),
        &format!("#!/usr/bin/env bash\n{AWAIT}{LANDED}case $1 in\n1) await main-at-start ;;\n2) start=$(main); printf '%s\\n' \"$start\" >\"$LAND_TRACE/main-at-start.tmp\"; mv \"$LAND_TRACE/main-at-start.tmp\" \"$LAND_TRACE/main-at-start\"; landed \"$start\"; main >\"$LAND_TRACE/main-at-end\" ;;\nesac\n"),
    );
    let output = speculate_serve(&mut fixture, std::slice::from_ref(&b));
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2, "the speculative candidate lands without re-running");
    let first = &checks[0].0;
    assert_eq!(fixture.read("main-at-start").trim(), fixture.base, "the speculative check starts before the first candidate lands");
    assert_eq!(fixture.read("main-at-end").trim(), first, "the first candidate lands while the speculative check is in flight");
    assert!(fixture.read("checks").lines().nth(1).unwrap().ends_with(&format!("--check-candidate --base {first}")), "{}", fixture.read("checks"));
    assert_eq!(fixture.main(), checks[1].0);
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    let pushes = fixture.read("pushes");
    assert_eq!(pushes.lines().count(), 2);
    assert!(pushes.lines().nth(1).unwrap().contains(&format!("--force-with-lease=refs/heads/main:{first}")), "{pushes}");
    pushed_trees_were_checked(&fixture);
}

fn wait_for(path: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !path.exists() {
        assert!(std::time::Instant::now() < deadline, "{} never appeared", path.display());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn stacked_serve(fixture: &mut Fixture, depth: &str, arrivals: &[(usize, &[PathBuf])]) -> Output {
    let batches: Vec<(usize, Vec<(PathBuf, Ticket)>)> = arrivals.iter().map(|(after, lanes)| (*after, lanes.iter().map(|lane| fixture.ticket(lane, None)).collect())).collect();
    let child = fixture.command().args(["--depth", depth]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    for (after, tickets) in &batches {
        wait_for(&fixture.trace.join(format!("started-{after}")));
        for (path, ticket) in tickets {
            queue::write(path, ticket).unwrap();
        }
        fs::write(fixture.trace.join(format!("arrived-{after}")), "").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    eprintln!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    output
}

fn stacked(output: &Output) -> usize {
    String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.starts_with("north land: speculative candidate ") && line.contains(" carries ")).count()
}

const STACKED_HOOK: &str = "#!/usr/bin/env bash\nawait() { for ((i = 0; i < 1200; i++)); do [ -e \"$LAND_TRACE/$1\" ] && return 0; sleep 0.05; done; return 1; }\ndate +%s.%N >\"$LAND_TRACE/start-$1\"\ntouch \"$LAND_TRACE/started-$1\"\n";

#[test]
fn speculate_depth_three() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b", "c", "d"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(&fixture.trace.join("hook"), &format!("{STACKED_HOOK}[ \"$1\" -gt 3 ] || await started-3\nsleep 5\ndate +%s.%N >\"$LAND_TRACE/end-$1\"\n"));
    let output = stacked_serve(&mut fixture, "3", &[(1, &lanes[1..2]), (2, &lanes[2..])]);
    assert!(output.status.success());
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c", "change from d"]);
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "each candidate is checked once: {checks:?}");
    assert_eq!(checks[0].1, fixture.base);
    assert_eq!(checks[1].1, checks[0].0, "the second candidate is stacked on the first");
    assert_eq!(checks[2].1, checks[1].0, "the third candidate is stacked on the second");
    assert_eq!(stacked(&output), 2, "two candidates were stacked beside the first");
    let (starts, ends): (Vec<f64>, Vec<f64>) = (1..=3).map(|n| (stamp(&fixture, &format!("start-{n}")), stamp(&fixture, &format!("end-{n}")))).unzip();
    let length = starts.iter().zip(&ends).map(|(start, end)| end - start).fold(0.0, f64::max);
    let span = ends.iter().copied().fold(0.0, f64::max) - starts[0];
    assert!(starts.iter().copied().fold(0.0, f64::max) < ends.iter().copied().fold(f64::MAX, f64::min), "all three checks were in flight at once: {starts:?} {ends:?}");
    assert!(span <= 1.2 * length, "three stacked checks took {span:.2} s from the first start, over 1.2 check lengths of {length:.2} s");
    assert_eq!(fixture.main(), checks[2].0);
    assert_eq!(fixture.read("pushes").lines().count(), 3);
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_depth_three_red_middle() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = [("a", "a.txt"), ("b", "fail.txt"), ("c", "c.txt")].iter().map(|(name, file)| fixture.lane(name, file, name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(
        &fixture.trace.join("hook"),
        &format!("{STACKED_HOOK}case $1 in\n1) ! await stopped || touch \"$LAND_TRACE/stopped-in-flight\" ;;\n2) await started-3 ;;\n3) echo $$ >\"$LAND_TRACE/stacked-pid\"; sleep 300 ;;\nesac\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n"),
    );
    let watcher = {
        let trace = fixture.trace.clone();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            while std::time::Instant::now() < deadline {
                if let Ok(pid) = fs::read_to_string(trace.join("stacked-pid"))
                    && !Path::new(&format!("/proc/{}", pid.trim())).exists()
                {
                    fs::write(trace.join("stopped"), "").unwrap();
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            false
        })
    };
    let output = stacked_serve(&mut fixture, "3", &[(1, &lanes[1..2]), (2, &lanes[2..])]);
    assert!(output.status.success());
    assert!(watcher.join().unwrap(), "the check stacked on the red candidate was stopped");
    assert!(fixture.trace.join("stopped-in-flight").exists(), "the stacked check was stopped while the first candidate was still checked, not when its turn to land came");
    let checks = fixture.checks();
    assert!(checks.len() >= 4, "{checks:?}");
    assert_eq!(checks[1].1, checks[0].0);
    assert_eq!(checks[2].1, checks[1].0, "c was stacked on the red b");
    assert!(checks[3..].iter().all(|(_, base)| *base == checks[0].0), "after the red middle, its lanes are rebuilt on the new main: {checks:?}");
    let culprit = json_names(&fixture.queue().join("done")).into_iter().find(|name| name.ends_with("-lane-b.json")).unwrap();
    assert!(fixture.verdict(&fixture.queue().join(culprit)).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from a", "change from c"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "fail.txt"));
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_depth_one_waits() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(&fixture.trace.join("hook"), &format!("{STACKED_HOOK}[ \"$1\" != 1 ] || {{ await arrived-1; sleep 1; }}\n"));
    let output = stacked_serve(&mut fixture, "1", &[(1, &lanes[1..])]);
    assert!(output.status.success());
    assert_eq!(stacked(&output), 0, "depth 1 never speculates");
    assert!(!fixture.root.join("proj/worktrees/lander-2").exists());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[1].1, checks[0].0, "b is checked on main after a landed");
    assert_eq!(fixture.subjects(), ["change from a", "change from b"]);
    pushed_trees_were_checked(&fixture);
}

#[test]
fn speculate_depth_two_stacks_once() {
    let mut fixture = Fixture::new();
    let lanes: Vec<PathBuf> = ["a", "b", "c"].iter().map(|name| fixture.lane(name, &format!("{name}.txt"), name)).collect();
    fixture.enqueue(&lanes[0], None);
    executable(&fixture.trace.join("hook"), &format!("{STACKED_HOOK}[ \"$1\" -gt 2 ] || {{ await arrived-2; sleep 1; }}\n"));
    let output = stacked_serve(&mut fixture, "2", &[(1, &lanes[1..2]), (2, &lanes[2..])]);
    assert!(output.status.success());
    assert!(!fixture.root.join("proj/worktrees/lander-3").exists(), "depth 2 keeps one spare worktree");
    assert_eq!(stacked(&output), 1, "c waits for the next round instead of a second stacked candidate");
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "{checks:?}");
    assert_eq!(checks[1].1, checks[0].0);
    assert_eq!(checks[2].1, checks[1].0);
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c"]);
    pushed_trees_were_checked(&fixture);
}
