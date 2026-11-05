use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::{Fixture, executable, git};

const ALONE_WAIT: &str = r#"#!/usr/bin/env bash
! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >"$LAND_TRACE/verdict-$1"
[ "$1" -ge 2 ] && [ "$1" -le 5 ] || exit 0
date +%s%N >"$LAND_TRACE/start-$1"
for k in $(seq 400); do [ "$(ls "$LAND_TRACE"/start-* | wc -l)" = 4 ] && break; sleep 0.05; done
date +%s%N >"$LAND_TRACE/end-$1"
echo "safe-push: ✗ the pre-push checks failed in check $1; NOT pushed."
"#;

fn stamps(fixture: &Fixture, prefix: &str) -> Vec<u128> {
    (2..=5).map(|n| fixture.read(&format!("{prefix}-{n}")).trim().parse().unwrap()).collect()
}

fn culprit_lanes(fixture: &mut Fixture) -> Vec<PathBuf> {
    ["a", "b", "c", "d"]
        .iter()
        .map(|name| {
            let file = if *name == "c" { "fail.txt".to_owned() } else { format!("{name}.txt") };
            let lane = fixture.lane(name, &file, name);
            fixture.enqueue(&lane, None)
        })
        .collect()
}

#[test]
fn culprit_search_names_the_culprit_in_one_round() {
    let mut fixture = Fixture::new();
    let tickets = culprit_lanes(&mut fixture);
    executable(&fixture.trace.join("hook"), ALONE_WAIT);
    let output = fixture.serve();
    assert!(output.status.success());
    let checks = fixture.checks();
    assert_eq!(checks.len(), 6, "the refused candidate, four lanes alone, then the three green lanes together");
    assert!(checks[1..5].iter().all(|(_, base)| *base == fixture.base), "each lane is checked alone on the unmoved base");
    let (starts, ends) = (stamps(&fixture, "start"), stamps(&fixture, "end"));
    assert!(starts.iter().max() < ends.iter().min(), "the four single-lane checks overlapped: starts {starts:?} ends {ends:?}");
    let culprit = (2..=5).find(|n| Command::new("git").arg("-C").arg(&fixture.repo).args(["cat-file", "-e", &format!("{}:fail.txt", checks[n - 1].0)]).output().unwrap().status.success()).unwrap();
    let failure = fixture.verdict(&tickets[2]).unwrap();
    assert!(failure.starts_with("failed ") && failure.ends_with("-candidate.log"), "{failure}");
    let log = failure.trim_start_matches("failed ");
    assert!(fs::read_to_string(log).unwrap().contains(&format!("failed in check {culprit}")), "the culprit's own log: {log}");
    let fleet = fixture.fleet();
    let told: Vec<&String> = fleet.iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-c ")).collect();
    assert_eq!(told.len(), 1, "{fleet:?}");
    assert!(told[0].contains(&format!("safe-push: ✗ the pre-push checks failed in check {culprit}; NOT pushed.")) && told[0].contains(&format!("full log: {log}")), "{told:?}");
    let main = fixture.main();
    for ticket in [&tickets[0], &tickets[1], &tickets[3]] {
        assert_eq!(fixture.verdict(ticket), Some(format!("landed {main}")));
    }
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from d"]);
    assert_eq!(checks[5].0, main, "the three green lanes landed as one candidate");
    assert_eq!(fixture.read("pushes").lines().count(), 1);
    assert_eq!(git(&fixture.origin, &["rev-list", "--count", &format!("{}..main", fixture.base)]), "3");
    assert_eq!(fleet.len(), 4, "each lane is told once: {fleet:?}");
}

#[test]
fn culprit_search_checks_in_waves_of_the_free_slots() {
    let mut fixture = Fixture::new();
    let tickets = culprit_lanes(&mut fixture);
    let offload = fixture.root.join("offload");
    fs::create_dir_all(&offload).unwrap();
    for lock in ["proj-main.lock", "proj-main-1.lock"] {
        fs::write(offload.join(lock), "").unwrap();
    }
    let mut holder = Command::new("flock").args(["-x", offload.join("proj-main.lock").to_str().unwrap(), "sleep", "120"]).spawn().unwrap();
    while Command::new("flock").args(["-n", offload.join("proj-main.lock").to_str().unwrap(), "true"]).status().unwrap().success() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    executable(&fixture.trace.join("hook"), "#!/usr/bin/env bash\n! command git cat-file -e HEAD:fail.txt 2>/dev/null || echo refused >\"$LAND_TRACE/verdict-$1\"\n");
    let output = fixture.command().env("HOME", &fixture.root).env("NORTH_OFFLOAD_ROOT", &offload).output().unwrap();
    holder.kill().unwrap();
    holder.wait().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    assert!(stdout.contains("1 of 2 offload slots are free, so 4 checks run in waves of 1"), "{stdout}");
    assert!(fixture.verdict(&tickets[2]).unwrap().starts_with("failed "));
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from d"]);
}

#[test]
fn culprit_search_lands_the_green_prefix_when_every_lane_passes_alone() {
    let mut fixture = Fixture::new();
    let tickets: Vec<PathBuf> = ["a", "b"].iter().map(|name| {
        let lane = fixture.lane(name, &format!("{name}.txt"), name);
        fixture.enqueue(&lane, None)
    }).collect();
    fs::write(fixture.trace.join("verdict-1"), "refused https://github.com/o/r/actions/runs/1").unwrap();
    let output = fixture.serve();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let checks = fixture.checks();
    assert_eq!(checks.len(), 3, "the pair and each lane alone; the pair is known red and lane-a green alone, so no further check: {checks:?}");
    let main = fixture.main();
    assert_eq!(stdout.matches(&format!("the first 1 lanes pass together and lane-b breaks them; landing them as {main} and refusing it")).count(), 1, "{stdout}");
    assert_eq!(fixture.verdict(&tickets[0]), Some(format!("landed {main}")));
    assert!(checks.iter().any(|(head, _)| *head == main), "lane-a lands as the head it was checked alone at: {checks:?}");
    let refused = fixture.verdict(&tickets[1]).unwrap();
    assert!(refused.starts_with("failed interaction with lane-a: "), "{refused}");
    assert_eq!(fixture.subjects(), ["change from a"]);
}
