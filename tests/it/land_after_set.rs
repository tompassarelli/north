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

const NEXTEST: &str = "nextest run";
const NO_TESTS_REASON: &str = "# no-tests:";
const BINARY_TERM: &str = "binary_id(=";
const TEST_TERM: &str = " & test(=";

fn rust_files(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found
}

fn names_a_test_that_exists(root: &Path, krate: &str, path: &str) -> bool {
    let (modules, name) = path.rsplit_once("::").unwrap_or(("", path));
    let (declared, last) = (format!("fn {name}("), modules.rsplit("::").next().unwrap_or_default());
    let source = root.join("crates").join(krate).join("src");
    let texts: Vec<String> = rust_files(&source).iter().map(|file| std::fs::read_to_string(file).unwrap_or_default()).collect();
    texts.iter().any(|text| text.contains(&declared)) && (modules.is_empty() || texts.iter().any(|text| text.contains(&format!("mod {last}"))))
}

fn nextest_refusals(root: &Path, command: &str) -> Vec<String> {
    if !command.contains(NEXTEST) {
        return Vec::new();
    }
    let mut refusals = Vec::new();
    let reason = command.split_once(NO_TESTS_REASON).is_some_and(|(_, reason)| !reason.trim().is_empty());
    if !command.contains("--no-tests=fail") && !reason {
        refusals.push("a nextest line runs with --no-tests=fail, or ends in `# no-tests: REASON`".to_owned());
    }
    for term in command.split(BINARY_TERM).skip(1) {
        let Some((krate, rest)) = term.split_once(')') else { continue };
        let Some(path) = rest.strip_prefix(TEST_TERM).and_then(|rest| rest.split_once(')')).map(|(path, _)| path) else { continue };
        if !names_a_test_that_exists(root, krate, path) {
            refusals.push(format!("{krate} has no test {path}"));
        }
    }
    refusals
}

// KEEP(bug): the Rust habit-test line named north-guard after 021ea25a9 moved the tests to north-code-health, so its term selected nothing and passed, silently, until 35ec2ecfc (north#782)
#[test]
fn a_nextest_land_line_refuses_to_pass_on_no_tests_and_names_only_tests_that_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let live: Vec<String> = ["land.check", "land.after", "land.machine-bound"].iter().flat_map(|key| values(key)).flat_map(|entry| nextest_refusals(root, command(&entry)).into_iter().map(move |refusal| format!("{}: {refusal}", command(&entry).chars().take(80).collect::<String>()))).collect();
    assert_eq!(live, Vec::<String>::new());
    let stale = "cargo nextest run --locked --offline --no-tests=pass -p north-guard --lib -E 'binary_id(=north-guard) & test(=code_health::habits_tests::the_north_tree_keeps_its_rust_habits)'";
    assert_eq!(nextest_refusals(root, stale).len(), 2, "the stale filter of 021ea25a9 passed because it neither failed on no tests nor named a test that exists");
}
