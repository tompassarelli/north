use crate::loops::{Loop, REGISTRY, check, last_written, parse};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DECISION_SITE: &str = r"tuned::|cells_file|start_hand\(|eta_minutes\(|BUDGET_TIMES|model_effort|estimate_command|learned_minimum_runs|requested_memory_mib|Deadline::from_passes|Timings::at|fn batch\((&self\)|facts)|room::Rank|resident_cap|pressure_target::";

const AREAS: [&str; 7] = [
    "crates/north-dispatch/src/dispatch",
    "crates/north-dispatch/src/pacing/placement",
    "src/cli/fleet",
    "crates/north-core/src/capacity",
    "crates/north-capacity-host/src/capacity",
    "crates/north-landing/src/land",
    "crates/north-landing/src/landing",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") && !["tests", "fixtures"].iter().any(|test_only| path.to_string_lossy().contains(test_only)) {
            found.push(path);
        }
    }
}

fn decision_sites() -> BTreeSet<String> {
    let pattern = regex::Regex::new(DECISION_SITE).unwrap();
    let mut files = Vec::new();
    AREAS.iter().for_each(|area| sources(&root().join(area), &mut files));
    files
        .into_iter()
        .filter(|file| pattern.is_match(&std::fs::read_to_string(file).unwrap()))
        .map(|file| file.strip_prefix(root()).unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_loops_registry_covers_every_decision_site_behind_dispatch_launch_admission_and_the_lander() {
    let loops = parse(REGISTRY).unwrap();
    let listed: BTreeSet<String> = loops.iter().flat_map(|found| found.sites.iter().cloned()).collect();
    let sites = decision_sites();
    assert!(sites.len() >= 20, "the decision-site scan found only {sites:?}");
    assert_eq!(sites.difference(&listed).collect::<Vec<_>>(), Vec::<&String>::new());
    assert_eq!(listed.iter().filter(|site| !root().join(site).is_file()).collect::<Vec<_>>(), Vec::<&String>::new());
    let names: BTreeSet<&str> = loops.iter().map(|found| found.name.as_str()).collect();
    for decision in ["tier", "effort", "eta", "capacity-estimate", "check-deadline", "placement", "worker-concurrency", "landing-batch", "skill-routing"] {
        assert!(names.contains(decision), "the registry has no {decision} loop");
    }
}

const FIXTURE: &str = r#"
[[loop]]
name = "tier"
decision = "tier per category"
sites = ["a.rs"]
signal = "cost per kept box"
updated_by = "north tune"
cadence_hours = 24
record = "north/tune/cells.json"

[[loop]]
name = "deadline"
decision = "land check deadline"
sites = ["b.rs"]
signal = "p90 pass wall"
updated_by = "north land"
cadence_hours = 24
record = "safe-push/check-timings/passes"
"#;

fn written(dir: &Path, record: &str, at: SystemTime) {
    let path = dir.join(record);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(&path).unwrap().set_modified(at).unwrap();
}

#[test]
fn loops_check_fails_a_stale_loop_and_passes_a_fresh_one() {
    let dir = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    written(dir.path(), "north/tune/cells.json", now - Duration::from_secs(3 * 3_600));
    written(dir.path(), "safe-push/check-timings/passes", now - Duration::from_secs(49 * 3_600));
    let checked = check(&parse(FIXTURE).unwrap(), &|record| last_written(dir.path(), record), now);
    assert_eq!(checked.failing, vec!["deadline".to_owned()]);
    assert!(checked.lines[0].starts_with("tier: ok: north tune updated 3 h ago"), "{:?}", checked.lines);
    assert!(checked.lines[1].starts_with("deadline: FAIL stale: north land last updated 49 h ago"), "{:?}", checked.lines);
    assert_eq!(checked.lines[2], "loops: 1 of 2 failing");
}

#[test]
fn loops_check_fails_a_loop_never_updated_or_without_an_outcome_signal() {
    let dir = tempfile::tempdir().unwrap();
    let unsignalled = Loop { name: "effort".into(), decision: "effort".into(), sites: vec!["a.rs".into()], signal: None, updated_by: "by hand".into(), cadence_hours: 24, record: None, owner: Some("north#112 Effort is an arm".into()) };
    let mut loops = parse(FIXTURE).unwrap();
    loops.truncate(1);
    loops.push(unsignalled);
    let checked = check(&loops, &|record| last_written(dir.path(), record), SystemTime::now());
    assert_eq!(checked.failing, vec!["tier".to_owned(), "effort".to_owned()]);
    assert_eq!(checked.lines[0], "tier: FAIL never updated: no north/tune/cells.json");
    assert_eq!(checked.lines[1], "effort: FAIL no outcome signal; by hand sets it; owner north#112 Effort is an arm");
}

#[test]
fn a_loop_without_a_signal_needs_an_owner() {
    let unowned = FIXTURE.replacen("signal = \"cost per kept box\"\n", "", 1).replacen("record = \"north/tune/cells.json\"\n", "", 1);
    assert_eq!(parse(&unowned), Err("loop tier has no signal and no owner to close it".to_owned()));
}
