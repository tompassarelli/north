use serde_json::Value;

use super::estimate::{CapacityQuery, CapacityRun, TaskSignature, estimate_capacity};

const USAGE: &str = include_str!("fixtures/capacity-usage.recorded.jsonl");
const DURATION: &str = include_str!("fixtures/capacity-duration.recorded.jsonl");
const WEEK_MS: i64 = 7 * 86_400_000;

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn recorded(fixture: &str) -> Vec<CapacityRun> {
    let mut runs: Vec<CapacityRun> = fixture
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: Value = serde_json::from_str(line).expect("recorded run parses");
            let signature = &row["signature"];
            CapacityRun {
                at: text(&row["at"]).expect("run time"),
                signature: TaskSignature {
                    family: text(&signature["family"]).expect("family"),
                    repo: text(&signature["repo"]),
                    package: text(&signature["package"]),
                    class: text(&signature["class"]).expect("class"),
                    exact: signature["exact"].as_bool().expect("exact"),
                },
                owner: text(&row["owner"]),
                shape: text(&row["shape"]),
                mean_cores: row["meanCores"].as_f64(),
                p90_cores: row["p90Cores"].as_f64(),
                peak_memory_mib: row["peakMemoryMiB"].as_f64(),
                io_read_bytes: row["ioReadBytes"].as_f64(),
                io_write_bytes: row["ioWriteBytes"].as_f64(),
                disk_busy_share: row["diskBusyShare"].as_f64(),
                wall_seconds: row["wallSeconds"].as_f64(),
                exit_code: row["exitCode"].as_f64(),
                host: None,
            }
        })
        .collect();
    runs.sort_by(|left, right| left.at.cmp(&right.at));
    runs
}

fn millis(at: &str) -> i64 {
    crate::foundation::clock::Timestamp::parse_rfc3339(at).expect("RFC 3339 run time").unix_millis()
}

/// Each run is estimated from the runs recorded strictly before its timestamp.
fn replay(runs: &[CapacityRun], mut judge: impl FnMut(&CapacityRun, &[CapacityRun])) {
    let mut start = 0;
    while start < runs.len() {
        let end = runs[start..].iter().position(|run| run.at != runs[start].at).map_or(runs.len(), |offset| start + offset);
        for run in &runs[start..end] {
            judge(run, &runs[..start]);
        }
        start = end;
    }
}

#[derive(Default)]
struct Cores {
    defaults: usize,
    scored: usize,
    covered: usize,
    over_reserved: usize,
}

#[test]
fn the_recorded_week_of_runs_is_covered_by_its_estimates_without_over_reserving() {
    let runs: Vec<CapacityRun> = recorded(USAGE).into_iter().filter(|run| !matches!(run.shape.as_deref(), Some("nix develop" | "nix shell"))).collect();
    let cutoff = millis(&runs.last().expect("recorded runs").at) - WEEK_MS;
    let mut cores = Cores::default();
    replay(&runs, |run, earlier| {
        let Some(measured) = run.p90_cores.filter(|cores| cores.is_finite()) else { return };
        if millis(&run.at) < cutoff {
            return;
        }
        let query = CapacityQuery { owner: run.owner.clone(), shape: run.shape.clone() };
        let estimate = estimate_capacity(&run.signature, earlier, 24.0, &query);
        if estimate["matchLevel"] == "default" {
            cores.defaults += 1;
            return;
        }
        let predicted = estimate["cores"]["p90"].as_f64().expect("a learned estimate has a p90");
        cores.scored += 1;
        cores.covered += usize::from(measured <= 1.3 * predicted + 0.25);
        cores.over_reserved += usize::from(predicted > 2.0 * measured + 0.25);
    });
    let share = |count: usize| count as f64 / cores.scored as f64;
    println!("scored={} defaults={} covered={} over-reserved={}", cores.scored, cores.defaults, cores.covered, cores.over_reserved);
    assert!(cores.scored > 379, "only {} runs scored", cores.scored);
    assert!(share(cores.covered) >= 0.95, "coverage {}/{}", cores.covered, cores.scored);
    assert!(share(cores.over_reserved) < 0.2, "over-reservation {}/{}", cores.over_reserved, cores.scored);
}

#[test]
fn the_recorded_exact_runs_finish_within_their_p75_duration_estimate_often_enough() {
    let runs = recorded(DURATION);
    let (mut thin, mut scored, mut covered) = (0, 0, 0);
    replay(&runs, |run, earlier| {
        let estimate = estimate_capacity(&run.signature, earlier, 24.0, &CapacityQuery::default());
        let Some(p75) = estimate["duration"]["p75Seconds"].as_f64() else {
            thin += 1;
            return;
        };
        scored += 1;
        covered += usize::from(run.wall_seconds.expect("timed run") <= p75);
    });
    println!("uncalibrated={thin} scored={scored} covered={covered}");
    assert!(scored > 0 && covered as f64 / scored as f64 >= 0.72, "p75 coverage {covered}/{scored}");
}
