use super::{Loop, Plumbing, REGISTRY, parse, plumbing};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const AREAS: [&str; 7] = [
    "crates/north-dispatch/src/dispatch",
    "crates/north-dispatch/src/pacing/placement",
    "src/cli/fleet",
    "crates/north-core/src/capacity",
    "crates/north-capacity-host/src/capacity",
    "crates/north-landing/src/land",
    "crates/north-landing/src/landing",
];

const CONSTANT: &str = r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?const ([A-Z][A-Z0-9_]*): (?:u8|u16|u32|u64|u128|usize|i8|i16|i32|i64|i128|isize|f32|f64|Duration)\b";
const DEFAULT_KEY: &str = r#"\b\w*default\w*\(\s*"([a-z0-9_]+)""#;

pub struct Found {
    pub constants: BTreeSet<String>,
    pub defaults: BTreeSet<String>,
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !name.contains("fixtures") && !name.contains("tests") {
                sources(&path, found);
            }
        } else if name.ends_with(".rs") && !name.contains("tests") && !name.contains("fixtures") {
            found.push(path);
        }
    }
}

pub fn scan(root: &Path, areas: &[&str]) -> Found {
    let (constant, default) = (Regex::new(CONSTANT).unwrap(), Regex::new(DEFAULT_KEY).unwrap());
    let mut files = Vec::new();
    areas.iter().for_each(|area| sources(&root.join(area), &mut files));
    let mut found = Found { constants: BTreeSet::new(), defaults: BTreeSet::new() };
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        let relative = file.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        found.constants.extend(constant.captures_iter(text).map(|capture| format!("{relative}::{}", &capture[1])));
        found.defaults.extend(default.captures_iter(text).map(|capture| capture[1].to_owned()));
    }
    found
}

pub fn problems(found: &Found, loops: &[Loop], plumbing: &Plumbing) -> Vec<String> {
    let named_constants: BTreeSet<&String> = loops.iter().flat_map(|found| &found.constants).chain(&plumbing.constants).collect();
    let named_defaults: BTreeSet<&String> = loops.iter().flat_map(|found| &found.defaults).chain(&plumbing.defaults).collect();
    let mut problems = Vec::new();
    problems.extend(found.constants.iter().filter(|name| !named_constants.contains(name)).map(|name| format!("constant {name} is in no loop's constants and not in plumbing")));
    problems.extend(found.defaults.iter().filter(|key| !named_defaults.contains(key)).map(|key| format!("defaults key {key} is in no loop's defaults and not in plumbing")));
    problems.extend(named_constants.iter().filter(|name| !found.constants.contains(**name)).map(|name| format!("listed constant {name} is not in any scanned file")));
    problems.extend(named_defaults.iter().filter(|key| !found.defaults.contains(**key)).map(|key| format!("listed defaults key {key} is not read in any scanned file")));
    problems
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_numeric_constant_and_defaults_key_in_scope_is_named_by_a_loop_or_plumbing() {
    let found = scan(&root(), &AREAS);
    assert!(found.constants.len() >= 100 && found.defaults.len() >= 20, "the scan found {} constants and {} defaults keys", found.constants.len(), found.defaults.len());
    assert_eq!(problems(&found, &parse(REGISTRY).unwrap(), &plumbing(REGISTRY).unwrap()), Vec::<String>::new());
}

#[test]
fn a_constant_added_under_a_scanned_file_and_named_by_nothing_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates/x/src/dispatch")).unwrap();
    std::fs::write(
        dir.path().join("crates/x/src/dispatch/slots.rs"),
        "pub const NAMED: u64 = 1;\nconst PROBE: u64 = 7;\nconst LABEL: &str = \"x\";\nfn f() { default(\"fresh_key\"); default_mib(\"known_key\"); }\n#[cfg(test)]\nmod tests { const IN_TEST: u64 = 1; }\n",
    )
    .unwrap();
    let found = scan(dir.path(), &["crates/x/src/dispatch"]);
    let registry = "[[loop]]\nname = \"slots\"\ndecision = \"d\"\nsites = [\"crates/x/src/dispatch/slots.rs\"]\nsignal = \"s\"\nupdated_by = \"u\"\ncadence_hours = 24\nrecord = \"r\"\nconstants = [\"crates/x/src/dispatch/slots.rs::NAMED\"]\ndefaults = [\"known_key\"]\n[plumbing]\nconstants = []\ndefaults = []\n";
    let missing = problems(&found, &parse(registry).unwrap(), &plumbing(registry).unwrap());
    assert_eq!(missing, vec![
        "constant crates/x/src/dispatch/slots.rs::PROBE is in no loop's constants and not in plumbing".to_owned(),
        "defaults key fresh_key is in no loop's defaults and not in plumbing".to_owned(),
    ]);
    let named = registry.replace("constants = []", "constants = [\"crates/x/src/dispatch/slots.rs::PROBE\"]").replace("defaults = []", "defaults = [\"fresh_key\"]");
    assert_eq!(problems(&found, &parse(&named).unwrap(), &plumbing(&named).unwrap()), Vec::<String>::new());
}

#[test]
fn a_listed_constant_that_no_longer_exists_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a")).unwrap();
    std::fs::write(dir.path().join("a/b.rs"), "fn f() {}\n").unwrap();
    let found = scan(dir.path(), &["a"]);
    let stale = Plumbing { constants: vec!["a/b.rs::GONE".into()], defaults: vec!["gone_key".into()] };
    assert_eq!(problems(&found, &[], &stale).len(), 2);
}

const DECISION_SITE: &str = r"tuned::|cells_file|start_hand\(|eta_minutes\(|BUDGET_TIMES|model_effort|estimate_command|learned_minimum_runs|requested_memory_mib|Deadline::from_passes|Timings::at|fn batch\((&self\)|facts)|room::Rank|resident_cap|pressure_target::";

fn decision_sites() -> BTreeSet<String> {
    let pattern = Regex::new(DECISION_SITE).unwrap();
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
