use std::path::PathBuf;
use std::process::Output;

use north_landing::land::queue::{self, Ticket};

use super::{Fixture, executable, git, json_names, pushed_trees_were_checked};

const AWAIT: &str = "await() { for ((i = 0; i < 400; i++)); do [ -e \"$LAND_TRACE/$1\" ] && return 0; sleep 0.05; done; return 1; }\n";
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

