use std::path::PathBuf;

use super::{Fixture, executable, git};

const SPREAD_NORTH: &str = r#"#!/usr/bin/env bash
if [ "$1 $2 $3" = "capacity offload choose" ]; then
  shift 4
  taken=" $* "
  free() { local used=${taken//"--taken $1 "/$'\n'}; used=$(( $(printf '%s' "$used" | wc -l) )); echo $(( $2 - used )); }
  best="" most=0
  for pair in stub-a:2 stub-b:2 "$(cat /proc/sys/kernel/hostname):1"; do
    left=$(free "${pair%:*}" "${pair#*:}")
    [ "$left" -le "$most" ] || { best=${pair%:*}; most=$left; }
  done
  [ -n "$best" ] || { echo "north capacity offload: no host has room: stub-a: 0 cores free" >&2; exit 1; }
  [ "$best" = "$(cat /proc/sys/kernel/hostname)" ] || echo "$best"
  exit 0
fi
if [ "$1 $2 $3" = "capacity offload run" ]; then
  host=$5; lane=$(command git log -1 --format=%s); lane=${lane##* }
  mkdir -p "$LAND_TRACE/hosts/$host"
  printf '%s\n' "$lane" >>"$LAND_TRACE/hosts/$host/ran"
  sleep 3
fi
exit 0
"#;

const SPREAD_CHECK: &str = r#"#!/usr/bin/env bash
lane=$(command git log -1 --format=%s); lane=${lane##* }
date +%s.%N >"$LAND_TRACE/start-$lane"
printf '%s' "${SAFE_PUSH_LAND_RUNNER-unset}" >"$LAND_TRACE/runner-$lane"
if [ -n "${SAFE_PUSH_LAND_RUNNER:-}" ]; then
  "$NORTH_LAND_NORTH" capacity offload run --host "$SAFE_PUSH_LAND_RUNNER" --base HEAD~1 --head HEAD --timeout 60 -- 'cargo test'
else
  sleep 3
fi
date +%s.%N >"$LAND_TRACE/end-$lane"
"#;

#[test]
fn prechecks_spread_over_hosts() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "5"]);
    git(&fixture.repo, &["config", "land.runner", "build-host"]);
    executable(&fixture.trace.join("north-hook"), SPREAD_NORTH);
    executable(&fixture.trace.join("hook"), SPREAD_CHECK);
    let names = ["a", "b", "c", "d", "e"];
    let tickets: Vec<PathBuf> = names
        .iter()
        .map(|name| {
            let lane = fixture.lane(name, &format!("{name}.txt"), name);
            fixture.enqueue(&lane, None)
        })
        .collect();
    let output = fixture.serve();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    eprintln!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let stamp = |kind: &str, name: &str| -> f64 { fixture.read(&format!("{kind}-{name}")).trim().parse().unwrap() };
    let last_start = names.iter().map(|name| stamp("start", name)).fold(f64::MIN, f64::max);
    let first_end = names.iter().map(|name| stamp("end", name)).fold(f64::MAX, f64::min);
    assert!(last_start < first_end, "all five prechecks ran at once: the last started {:.3} s after the first ended", last_start - first_end);
    let mut runners: Vec<String> = names.iter().map(|name| fixture.read(&format!("runner-{name}"))).collect();
    runners.sort();
    assert_eq!(runners, ["", "stub-a", "stub-a", "stub-b", "stub-b"], "one precheck here and two on each stub host, each pinned to its host");
    let first_checks: Vec<&str> = stdout.lines().filter(|line| line.contains(" checks alone on main@") && line.contains(" in build slot ")).collect();
    assert_eq!(first_checks.len(), 5, "{stdout}");
    for host in ["stub-a", "stub-b"] {
        assert_eq!(fixture.read(&format!("hosts/{host}/ran")).lines().count(), 2, "{host} ran two prechecks through north capacity offload run");
        assert_eq!(first_checks.iter().filter(|line| line.ends_with(&format!(" on {host}"))).count(), 2, "{stdout}");
    }
    for (ticket, name) in tickets.iter().zip(names) {
        assert!(fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed ")), "lane {name} has no landed verdict");
    }
    assert_eq!(fixture.verdict(&tickets[4]), Some(format!("landed {}", fixture.main())));
    assert_eq!(fixture.subjects(), ["change from a", "change from b", "change from c", "change from d", "change from e"]);
}
