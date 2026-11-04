//! A replay of 2026-11-05 00:30 to 01:30 (+0700), the hour main stood still while the lander refused
//! lanes that conflicted with each other. The fixture holds each lane then in flight with its start
//! (its first commit) and every file it changed against origin/main; no lane landed in that hour, so
//! each launch meets all the lanes started before it.
use super::*;

const LANES: &str = include_str!("../fixtures/staffing-overlap-2026-11-05.tsv");
const BEFORE_REGENERATE_DRIVERS: &str = "harness/skills/machine-capacity/scripts/machine-capacity.mjs linguist-generated=true\nstructure.toml merge=set\nharness/machine-bound-tests merge=set\n";
const WITH_REGENERATE_DRIVERS: &str = "harness/skills/machine-capacity/scripts/machine-capacity.mjs linguist-generated=true\nstructure.toml merge=set\nharness/machine-bound-tests merge=set\ndocs/features.md merge=set\ndocs/rationale/README.md merge=set\n";

struct Launch {
    lane: Lane,
    brief: String,
}

fn launches() -> Vec<Launch> {
    let mut launches: Vec<Launch> = LANES
        .lines()
        .map(|row| {
            let [name, task, start, files]: [&str; 4] = row.split('\t').collect::<Vec<_>>().try_into().expect("four columns");
            let files: BTreeSet<String> = files.split(',').map(str::to_owned).collect();
            let brief = format!("Item: {task}\nFiles: {}\n", files.iter().cloned().collect::<Vec<_>>().join(", "));
            let lane = Lane { name: name.into(), task: (!task.is_empty()).then(|| task.into()), since_ms: start.parse::<u64>().expect("start seconds") * 1000, files };
            Launch { lane, brief }
        })
        .collect();
    launches.sort_by_key(|launch| launch.lane.since_ms);
    launches
}

fn verdicts(attributes: &str) -> Vec<(String, Verdict)> {
    let driven = MergeDriven::parse(attributes);
    let all = launches();
    all.iter()
        .enumerate()
        .map(|(at, launch)| {
            let in_flight: Vec<Lane> = all[..at].iter().map(|earlier| earlier.lane.clone()).collect();
            (launch.lane.name.clone(), assess(&declared_files(&launch.brief), &in_flight, &|file| driven.covers(file), Policy::default()))
        })
        .collect()
}

fn verdict_of<'a>(verdicts: &'a [(String, Verdict)], lane: &str) -> &'a Verdict {
    &verdicts.iter().find(|(name, _)| name == lane).expect("lane in the replay").1
}

#[test]
fn the_replay_queues_the_launch_that_met_two_lanes_on_land_serve_and_warns_the_one_that_met_one() {
    let verdicts = verdicts(BEFORE_REGENERATE_DRIVERS);
    assert!(matches!(verdict_of(&verdicts, "n360-refusal-reason"), Verdict::Clear));
    let Verdict::Warn(found) = verdict_of(&verdicts, "n360-s8-app-push") else { panic!("one lane on land_serve warns") };
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].lane, "n360-refusal-reason");
    assert!(found[0].files.iter().any(|file| file == "tests/it/land_serve.rs") && found[0].files.iter().any(|file| file == "crates/north-landing/src/land/serve.rs"));
    let Verdict::Queue { behind, hot, overlaps } = verdict_of(&verdicts, "etxtbsy-helper") else { panic!("two lanes on land_serve queue the launch") };
    assert_eq!((behind.lane.as_str(), behind.task.as_deref()), ("n360-refusal-reason", Some("north#360")));
    assert_eq!(hot, &["tests/it/land_serve.rs"]);
    assert_eq!(overlaps.len(), 2);
}

#[test]
fn the_replay_queues_only_launches_whose_hot_file_has_an_earlier_lane_with_a_task() {
    let queued: Vec<String> = verdicts(BEFORE_REGENERATE_DRIVERS).into_iter().filter(|(_, verdict)| matches!(verdict, Verdict::Queue { .. })).map(|(lane, _)| lane).collect();
    assert_eq!(queued, ["etxtbsy-helper"]);
}

#[test]
fn the_replay_finds_docs_features_on_every_docs_lane_after_the_first_and_flags_the_hot_ones() {
    let verdicts = verdicts(BEFORE_REGENERATE_DRIVERS);
    let on_features = |lane: &str| verdict_of(&verdicts, lane).overlaps().iter().filter(|overlap| overlap.files.iter().any(|file| file == "docs/features.md")).count();
    let counts: Vec<usize> = ["cloud-handback-land", "docs-3a-features", "docs-3c-features", "docs-3b-features", "docs-7-concepts"].iter().map(|lane| on_features(lane)).collect();
    assert_eq!(counts, [0, 1, 2, 3, 4]);
    assert!(matches!(verdict_of(&verdicts, "docs-7-concepts"), Verdict::Warn(found) if found.len() >= 4));
}

#[test]
fn the_regenerate_drivers_of_the_docs_lane_remove_the_features_index_from_every_overlap() {
    let verdicts = verdicts(WITH_REGENERATE_DRIVERS);
    for (lane, verdict) in &verdicts {
        assert!(verdict.overlaps().iter().all(|overlap| !overlap.files.iter().any(|file| file == "docs/features.md")), "{lane}");
    }
}

#[test]
fn the_fixture_holds_the_lanes_that_changed_the_hot_files() {
    let changing = |file: &str| launches().iter().filter(|launch| launch.lane.files.contains(file)).count();
    assert_eq!(changing("tests/it/land_serve.rs"), 3);
    assert_eq!(changing("docs/features.md"), 5);
    assert_eq!(changing("crates/north-landing/src/land/serve.rs"), 2);
}
