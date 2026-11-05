use std::collections::{BTreeSet, HashMap, VecDeque};

use serde_json::Value;

use super::estimate::{CapacityQuery, CapacityRun, TaskSignature, estimate_capacity};
use super::policy::{owner_prefix, sizing_runs};

const USAGE: &str = include_str!("fixtures/capacity-usage.recorded.jsonl");
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

#[derive(Clone, PartialEq, Eq, Hash)]
enum Pick {
    Exact(String, Option<String>, Option<String>, String),
    Timed(String, Option<String>, Option<String>, String),
    Seen(String, Option<String>, Option<String>),
    OwnerShape(String, String),
    PrefixShapeClass(String, String, String),
    FamilyRepo(String, String),
    Family(String),
}

fn picks(signature: &TaskSignature, owner: Option<&String>, shape: Option<&String>) -> Vec<Pick> {
    let TaskSignature { family, repo, package, class, .. } = signature;
    let mut picks = vec![
        Pick::Exact(family.clone(), repo.clone(), package.clone(), class.clone()),
        Pick::Timed(family.clone(), repo.clone(), package.clone(), class.clone()),
        Pick::Seen(family.clone(), repo.clone(), package.clone()),
        Pick::Family(family.clone()),
    ];
    if let (Some(owner), Some(shape)) = (owner, shape) {
        picks.push(Pick::OwnerShape(owner.clone(), shape.clone()));
        picks.push(Pick::PrefixShapeClass(owner_prefix(owner).to_owned(), shape.clone(), class.clone()));
    }
    if let Some(repo) = repo {
        picks.push(Pick::FamilyRepo(family.clone(), repo.clone()));
    }
    picks
}

#[derive(Default)]
struct Newest {
    indexed: usize,
    picks: HashMap<Pick, VecDeque<usize>>,
}

impl Newest {
    fn advance(&mut self, earlier: &[CapacityRun]) {
        for (index, run) in earlier.iter().enumerate().skip(self.indexed) {
            let valid = run.p90_cores.is_some_and(|cores| cores.is_finite() && cores >= 0.0);
            let timed = run.wall_seconds.is_some_and(|seconds| seconds.is_finite() && seconds > 0.0) && run.exit_code.unwrap_or(0.0) == 0.0;
            for pick in picks(&run.signature, run.owner.as_ref(), run.shape.as_ref()) {
                let applies = match pick {
                    Pick::Exact(..) | Pick::Seen(..) => valid && run.signature.exact,
                    Pick::Timed(..) => timed && run.signature.exact,
                    Pick::Family(ref family) => valid && family != "other",
                    Pick::OwnerShape(..) | Pick::PrefixShapeClass(..) | Pick::FamilyRepo(..) => valid,
                };
                if applies {
                    let newest = self.picks.entry(pick).or_default();
                    newest.push_back(index);
                    if newest.len() > sizing_runs() {
                        newest.pop_front();
                    }
                }
            }
        }
        self.indexed = earlier.len();
    }

    fn history(&self, earlier: &[CapacityRun], run: &CapacityRun, query: &CapacityQuery) -> Vec<CapacityRun> {
        let indices: BTreeSet<usize> = picks(&run.signature, query.owner.as_ref(), query.shape.as_ref()).iter().filter_map(|pick| self.picks.get(pick)).flatten().copied().collect();
        indices.into_iter().map(|index| earlier[index].clone()).collect()
    }
}

#[derive(Default)]
struct Cores {
    defaults: usize,
    scored: usize,
    covered: usize,
    over_reserved: usize,
}

// KEEP(bug): recorded replay: estimates cover p90
#[test]
fn the_recorded_week_of_runs_is_covered_by_its_estimates_without_over_reserving() {
    let runs: Vec<CapacityRun> = recorded(USAGE).into_iter().filter(|run| !matches!(run.shape.as_deref(), Some("nix develop" | "nix shell"))).collect();
    let cutoff = millis(&runs.last().expect("recorded runs").at) - WEEK_MS;
    let mut cores = Cores::default();
    let mut newest = Newest::default();
    let mut estimated = 0_usize;
    replay(&runs, |run, earlier| {
        newest.advance(earlier);
        let Some(measured) = run.p90_cores.filter(|cores| cores.is_finite()) else { return };
        if millis(&run.at) < cutoff {
            return;
        }
        let query = CapacityQuery { owner: run.owner.clone(), shape: run.shape.clone() };
        let estimate = estimate_capacity(&run.signature, &newest.history(earlier, run, &query), 24.0, &query);
        if estimated.is_multiple_of(100) {
            assert_eq!(estimate, estimate_capacity(&run.signature, earlier, 24.0, &query), "estimate from the newest runs at {}", run.at);
        }
        estimated += 1;
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
