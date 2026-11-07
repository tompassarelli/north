use std::path::Path;
use std::process::Command;

const SEPARATOR: &str = " :: ";
const ENQUEUE: &str = "@enqueue";
const OWNED_BY_NORTH714: [&str; 2] = ["land tests --select changed --only portable", "land scan test-admission run"];

fn values(key: &str) -> Vec<String> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".safe-push");
    let output = Command::new("git").args(["config", "-f"]).arg(file).args(["--get-all", key]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

fn command(entry: &str) -> &str {
    entry.split_once(SEPARATOR).map_or(entry, |(_, command)| command)
}

// KEEP(bug): after the north-land-scans and north-landing-gates splits four land lines still named --code paths that no longer existed, which read as covered by the live runtime, so head-north ran the old checker (north#714)
#[test]
fn every_code_path_a_land_line_names_exists_in_the_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let missing: Vec<String> = ["land.check", "land.after", "land.machine-bound"]
        .iter()
        .flat_map(|key| values(key))
        .flat_map(|entry| {
            let words: Vec<String> = command(&entry).split_whitespace().map(str::to_owned).collect();
            let paths = words.iter().position(|word| word == "--code").and_then(|at| words.get(at + 1)).map(|list| list.split(',').map(str::to_owned).collect::<Vec<_>>());
            paths.into_iter().flatten().filter(|path| !root.join(path).exists()).collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(missing, Vec::<String>::new(), "a --code path that does not exist reads as covered by the live runtime; name the path the checker's code lives at now");
}

// KEEP(bug): the skill-size check (98d30d226) and module layers (086197b4b) ran only after the landing, so each lane turned main red though its candidate tree could have refused it (north#782)
#[test]
fn every_land_after_check_is_either_run_on_the_candidate_tree_too_or_marked_as_needing_the_landed_live_state() {
    let (checks, bound) = (values("land.check"), values("land.machine-bound"));
    let misplaced: Vec<String> = values("land.after")
        .iter()
        .filter(|entry| {
            let patterns = entry.split_once(SEPARATOR).map_or("", |(patterns, _)| patterns);
            let owned = OWNED_BY_NORTH714.iter().any(|owned| entry.contains(owned));
            !owned && !patterns.split(',').any(|pattern| pattern == ENQUEUE) && ![&checks, &bound].iter().any(|listed| listed.iter().any(|line| command(line) == command(entry)))
        })
        .map(|entry| command(entry).chars().take(120).collect())
        .collect();
    assert_eq!(misplaced, Vec::<String>::new(), "add each of these to land.check so the candidate's tree is checked before the landing, or list it under land.machine-bound with the live state it needs");
}
