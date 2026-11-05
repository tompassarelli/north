use super::*;
use crate::ops::docs::run;

const INDEX_FILE: &str = INDEX.file;

fn generate(root: &Path) -> Result<(), String> {
    INDEX.generate(root)
}

fn check(root: &Path) -> Result<(), String> {
    INDEX.check(root)
}

const HEADER: &str = "# North features\n\nIntro line.\n\nStanding paragraph.\n";

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "docs/features/_header.md", HEADER);
    write(root, "docs/features/01-app/_title.md", "## The `north` app\n");
    write(root, "docs/features/01-app/010-fleet.md", "- Fleet screen: `north fleet`.\n");
    write(root, "docs/features/01-app/020-work.md", " - Work items: `north work list`.\n");
    write(root, "docs/features/02-payoff/_title.md", "## Payoff\n");
    write(root, "docs/features/02-payoff/010-first.md", "First paragraph.\n");
    write(root, "docs/features/02-payoff/020-second.md", "Second paragraph.\n");
    dir
}

const EXPECTED: &str = "# North features\n\nIntro line.\n\nStanding paragraph.\n\n## The `north` app\n\n- Fleet screen: `north fleet`.\n - Work items: `north work list`.\n\n## Payoff\n\nFirst paragraph.\n\nSecond paragraph.\n";

#[test]
fn generate_joins_header_sections_and_entries_in_name_order() {
    let dir = fixture();
    generate(dir.path()).unwrap();
    assert_eq!(fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap(), EXPECTED);
}

#[test]
fn check_passes_on_a_generated_index() {
    let dir = fixture();
    generate(dir.path()).unwrap();
    check(dir.path()).unwrap();
    assert_eq!(run(&["features-index".into(), "--check".into(), "--root".into(), dir.path().display().to_string()]), 0);
}

#[test]
fn check_fails_on_a_planted_stale_index() {
    let dir = fixture();
    generate(dir.path()).unwrap();
    let index = dir.path().join(INDEX_FILE);
    fs::write(&index, EXPECTED.replace("Fleet screen", "Fleet display")).unwrap();
    assert!(check(dir.path()).unwrap_err().contains("stale"));
    assert_eq!(run(&["features-index".into(), "--check".into(), "--root".into(), dir.path().display().to_string()]), 1);
    generate(dir.path()).unwrap();
    check(dir.path()).unwrap();
}

#[test]
fn check_fails_when_a_feature_file_is_added_without_regenerating() {
    let dir = fixture();
    generate(dir.path()).unwrap();
    write(dir.path(), "docs/features/01-app/015-new.md", "- New feature.\n");
    assert!(check(dir.path()).is_err());
}

#[test]
fn check_fails_when_the_index_is_missing() {
    let dir = fixture();
    assert!(check(dir.path()).is_err());
}

#[test]
fn entries_without_a_trailing_newline_render_the_same() {
    let dir = fixture();
    write(dir.path(), "docs/features/01-app/010-fleet.md", "- Fleet screen: `north fleet`.");
    generate(dir.path()).unwrap();
    assert_eq!(fs::read_to_string(dir.path().join(INDEX_FILE)).unwrap(), EXPECTED);
}

#[test]
fn unknown_arguments_exit_with_usage() {
    assert_eq!(run(&["features-index".into(), "--bogus".into()]), 2);
    assert_eq!(run(&[]), 2);
}

#[test]
fn check_fails_naming_an_entry_over_the_cap_on_a_fresh_index() {
    let dir = fixture();
    write(dir.path(), "docs/features/02-payoff/030-long.md", &format!("{}\n", "x".repeat(1_299)));
    generate(dir.path()).unwrap();
    let error = check(dir.path()).unwrap_err();
    assert!(error.contains("030-long.md") && error.contains("1300 characters"), "{error}");
    assert_eq!(run(&["features-index".into(), "--check".into(), "--root".into(), dir.path().display().to_string()]), 1);
    write(dir.path(), "docs/features/02-payoff/030-long.md", &format!("{}\n", "x".repeat(1_199)));
    generate(dir.path()).unwrap();
    check(dir.path()).unwrap();
}
