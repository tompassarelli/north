use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const NORTH: &str = env!("CARGO_BIN_EXE_north");
const SETS: &str = include_str!("../../harness/merge-sets.toml");
const ATTRIBUTES: &str = include_str!("../../.gitattributes");

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git").current_dir(root).env("GIT_EDITOR", "true").args(args).output().unwrap()
}

fn git_ok(root: &Path, args: &[&str]) {
    let output = git(root, args);
    assert!(output.status.success(), "git {args:?}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git_ok(root, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git_ok(root, &["config", key, value]);
    }
    git_ok(root, &["config", "merge.set.driver", &format!("{NORTH} merge-driver set %O %A %B %P")]);
    fs::create_dir_all(root.join("harness")).unwrap();
    fs::write(root.join("harness/merge-sets.toml"), SETS).unwrap();
    fs::write(root.join(".gitattributes"), ATTRIBUTES).unwrap();
    for (path, text) in files {
        fs::write(root.join(path), text).unwrap();
    }
    git_ok(root, &["add", "-A"]);
    git_ok(root, &["commit", "-qm", "base"]);
    dir
}

fn lane(root: &Path, name: &str, path: &str, text: &str) {
    git_ok(root, &["checkout", "-q", "-b", name, "main"]);
    fs::write(root.join(path), text).unwrap();
    git_ok(root, &["commit", "-qam", name]);
}

fn rebase_second_onto_first(root: &Path) -> Output {
    git_ok(root, &["checkout", "-q", "main"]);
    git_ok(root, &["merge", "-q", "--ff-only", "first"]);
    git_ok(root, &["checkout", "-q", "second"]);
    git(root, &["rebase", "main"])
}

#[test]
fn two_lanes_that_each_append_a_line_to_an_allowlisted_list_rebase_with_both_lines_kept() {
    let dir = repo(&[("harness/machine-bound-tests", "# header\nbin/a.test.sh\n")]);
    let root = dir.path();
    lane(root, "first", "harness/machine-bound-tests", "# header\nbin/a.test.sh\nbin/b.test.sh\n");
    lane(root, "second", "harness/machine-bound-tests", "# header\nbin/a.test.sh\nbin/c.test.sh\n");
    let rebase = rebase_second_onto_first(root);
    assert!(rebase.status.success(), "{}{}", String::from_utf8_lossy(&rebase.stdout), String::from_utf8_lossy(&rebase.stderr));
    assert_eq!(fs::read_to_string(root.join("harness/machine-bound-tests")).unwrap(), "# header\nbin/a.test.sh\nbin/b.test.sh\nbin/c.test.sh\n");
}

#[test]
fn two_lanes_that_each_add_a_module_dependency_to_one_structure_toml_line_rebase_with_both_kept() {
    let base = "[[module]]\nname = \"guard\"\nmay_use = [\"ledger\", \"shell\"]\n";
    let dir = repo(&[("structure.toml", base)]);
    let root = dir.path();
    lane(root, "first", "structure.toml", &base.replace("\"shell\"", "\"fleet\", \"shell\""));
    lane(root, "second", "structure.toml", &base.replace("\"shell\"", "\"ops\", \"usage\", \"shell\""));
    let rebase = rebase_second_onto_first(root);
    assert!(rebase.status.success(), "{}{}", String::from_utf8_lossy(&rebase.stdout), String::from_utf8_lossy(&rebase.stderr));
    let merged = fs::read_to_string(root.join("structure.toml")).unwrap();
    for entry in ["ledger", "shell", "fleet", "ops", "usage"] {
        assert_eq!(merged.matches(&format!("\"{entry}\"")).count(), 1, "{merged}");
    }
}

#[test]
fn an_ordered_guard_array_outside_the_allowlist_stays_a_conflict() {
    let base = "const GUARDS: &[&str] = &[\n    \"one\",\n    \"two\",\n];\n";
    let dir = repo(&[("hook.rs", base)]);
    let root = dir.path();
    lane(root, "first", "hook.rs", &base.replace("    \"two\",\n", "    \"two\",\n    \"first\",\n"));
    lane(root, "second", "hook.rs", &base.replace("    \"two\",\n", "    \"two\",\n    \"second\",\n"));
    let rebase = rebase_second_onto_first(root);
    assert!(!rebase.status.success());
    assert!(fs::read_to_string(root.join("hook.rs")).unwrap().contains("<<<<<<<"));
}

#[test]
fn a_duplicate_entry_added_by_both_lanes_stays_a_conflict_with_markers() {
    let dir = repo(&[("harness/machine-bound-tests", "bin/a.test.sh\n")]);
    let root = dir.path();
    lane(root, "first", "harness/machine-bound-tests", "bin/a.test.sh\nbin/b.test.sh\n");
    lane(root, "second", "harness/machine-bound-tests", "bin/a.test.sh\nbin/b.test.sh\nbin/c.test.sh\n");
    let rebase = rebase_second_onto_first(root);
    assert!(!rebase.status.success());
    assert!(fs::read_to_string(root.join("harness/machine-bound-tests")).unwrap().contains("<<<<<<<"));
}

fn add_feature(root: &Path, name: &str, path: &str) {
    git_ok(root, &["checkout", "-q", "-B", name, "main"]);
    fs::write(root.join(path), format!("- {name}: `north {name}`.\n")).unwrap();
    let generate = Command::new(NORTH).current_dir(root).args(["docs", "features-index"]).output().unwrap();
    assert!(generate.status.success(), "{}", String::from_utf8_lossy(&generate.stderr));
    git_ok(root, &["add", "-A"]);
    git_ok(root, &["commit", "-qm", name]);
}

#[test]
fn a_lane_adding_a_feature_beside_one_main_added_rebases_through_continue_with_a_fresh_index() {
    let dir = repo(&[]);
    let root = dir.path();
    fs::create_dir_all(root.join("docs/features/01-app")).unwrap();
    fs::write(root.join("docs/features/_header.md"), "# Features\n").unwrap();
    fs::write(root.join("docs/features/01-app/_title.md"), "## App\n").unwrap();
    add_feature(root, "main", "docs/features/01-app/010-fleet.md");
    add_feature(root, "first", "docs/features/01-app/020-work.md");
    add_feature(root, "second", "docs/features/01-app/030-docs.md");
    git_ok(root, &["checkout", "-q", "main"]);
    git_ok(root, &["merge", "-q", "--ff-only", "first"]);
    git_ok(root, &["checkout", "-q", "second"]);
    let rebase = git(root, &["rebase", "main"]);
    let finish = Command::new(NORTH).current_dir(root).args(["merge-driver", "continue"]).output().unwrap();
    assert!(!rebase.status.success(), "the generated index should stop the rebase for continue");
    assert!(finish.status.success(), "{}", String::from_utf8_lossy(&finish.stderr));
    assert!(!root.join(".git/rebase-merge").exists());
    let check = Command::new(NORTH).current_dir(root).args(["docs", "features-index", "--check"]).output().unwrap();
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    let index = fs::read_to_string(root.join("docs/features.md")).unwrap();
    assert!(index.contains("- first:") && index.contains("- second:"), "{index}");
}

#[test]
fn two_lanes_that_each_add_a_module_to_the_fleet_mod_list_rebase_with_both_kept() {
    const FLEET: &str = "crates/north-core/src/fleet/mod.rs";
    let base = include_str!("../../crates/north-core/src/fleet/mod.rs");
    let dir = repo(&[]);
    let root = dir.path();
    fs::create_dir_all(root.join("crates/north-core/src/fleet")).unwrap();
    fs::write(root.join(FLEET), base).unwrap();
    git_ok(root, &["add", "-A"]);
    git_ok(root, &["commit", "-qm", "fleet"]);
    lane(root, "first", FLEET, &format!("{base}pub mod first_cut;\n"));
    lane(root, "second", FLEET, &format!("{base}mod second_cut;\n"));
    let rebase = rebase_second_onto_first(root);
    assert!(rebase.status.success(), "{}{}", String::from_utf8_lossy(&rebase.stdout), String::from_utf8_lossy(&rebase.stderr));
    assert_eq!(fs::read_to_string(root.join(FLEET)).unwrap(), format!("{base}mod second_cut;\npub mod first_cut;\n"));
}
