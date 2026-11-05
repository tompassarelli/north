use super::rules::Rules;
use super::verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

const STRUCTURE: &str = r#"
[ignore]
paths = ["crates/*/src/fixtures/**"]

[[module]]
name = "usage"
paths = ["crates/north-core/src/usage/**"]
may_use = []

[[module]]
name = "fleet"
paths = ["crates/north-core/src/fleet/**"]
may_use = ["usage"]

[crate_dag]
re_exports = ["crates/*/src/lib.rs"]

[[crate_dag.crate]]
name = "north-pace"
paths = ["crates/*/src/usage/**"]
may_use = []

[[crate_dag.crate]]
name = "north-fleet"
paths = ["crates/*/src/fleet/**"]
may_use = ["north-pace"]
"#;

const MANIFEST: &str = "[package]\nname = \"north-core\"\nversion = \"0.1.0\"\n";
const LIB: &str = "pub mod fleet;\npub mod usage;\npub use std::fmt as foundation;\n";
const USAGE: &str = "pub mod decide;\n\npub fn read() -> u8 {\n    1\n}\n";
const USAGE_PATH: &str = "crates/north-core/src/usage/mod.rs";

fn git(repo: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let file = repo.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "step", "--allow-empty"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn lane(structure: &str, usage: &str) -> (Vec<String>, u8) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    let base = commit(repo, &[
        ("structure.toml", structure),
        ("crates/north-core/Cargo.toml", MANIFEST),
        ("crates/north-core/src/lib.rs", LIB),
        (USAGE_PATH, USAGE),
        ("crates/north-core/src/usage/decide.rs", "pub fn decide() {}\n"),
        ("crates/north-core/src/fleet/mod.rs", "pub mod records;\npub mod registry;\nuse crate::usage::decide::decide;\n"),
        ("crates/north-core/src/fleet/records.rs", "pub fn read_record() {}\n"),
        ("crates/north-core/src/fleet/registry.rs", "pub fn root_here() {}\n"),
    ]);
    let head = commit(repo, &[(USAGE_PATH, usage)]);
    verdict(repo, &base, &head).unwrap()
}

fn refused(lines: &[String], rule: &str) -> bool {
    lines.iter().any(|line| line.contains(rule))
}

#[test]
fn a_use_of_crate_fleet_in_usage_is_refused_naming_both_rules() {
    let (lines, code) = lane(STRUCTURE, &format!("use crate::fleet::records::read_record;\n{USAGE}"));
    assert_eq!(code, 1, "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with(&format!("✗ layer {USAGE_PATH}:1 new reference -> crates/north-core/src/fleet/records.rs: module usage may not use fleet"))), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with(&format!("✗ crate-dag {USAGE_PATH}:1 new reference -> crates/north-core/src/fleet/records.rs: planned crate north-pace may not use north-fleet"))), "{lines:?}");
    assert_eq!(lines.last().unwrap(), "✗ layers: 2 new reference(s) break structure.toml's module layers or crate DAG");
}

#[test]
fn an_inline_crate_fleet_path_is_refused_on_its_line() {
    let usage = format!("{USAGE}\npub fn root() {{\n    crate::fleet::registry::root_here();\n}}\n");
    let (lines, code) = lane(STRUCTURE, &usage);
    assert_eq!(code, 1, "{lines:?}");
    assert!(refused(&lines, &format!("✗ layer {USAGE_PATH}:8 new reference -> crates/north-core/src/fleet/registry.rs: module usage may not use fleet")), "{lines:?}");
    assert!(refused(&lines, "planned crate north-pace may not use north-fleet"), "{lines:?}");
}

#[test]
fn a_reference_already_on_the_base_is_not_refused_again() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    let breaking = format!("use crate::fleet::records::read_record;\n{USAGE}");
    let base = commit(repo, &[
        ("structure.toml", STRUCTURE),
        ("crates/north-core/Cargo.toml", MANIFEST),
        ("crates/north-core/src/lib.rs", LIB),
        (USAGE_PATH, &breaking),
        ("crates/north-core/src/usage/decide.rs", ""),
        ("crates/north-core/src/fleet/records.rs", "pub fn read_record() {}\n"),
        ("crates/north-core/src/fleet/registry.rs", "pub fn root_here() {}\n"),
    ]);
    let head = commit(repo, &[(USAGE_PATH, &format!("{breaking}\npub fn more() {{\n    crate::fleet::registry::root_here();\n}}\n"))]);
    let (lines, code) = verdict(repo, &base, &head).unwrap();
    assert_eq!((code, lines.last().unwrap().as_str()), (0, "layers: no new reference breaks structure.toml's module layers or crate DAG"), "{lines:?}");
    fs::remove_file(repo.join(USAGE_PATH)).unwrap();
    let moved = commit(repo, &[("crates/north-core/src/usage/shell.rs", &breaking)]);
    assert!(git(repo, &["diff", "-M", "--name-status", &base, &moved]).starts_with('R'));
    let (lines, code) = verdict(repo, &base, &moved).unwrap();
    assert_eq!(code, 0, "{lines:?}");
}

#[test]
fn allowed_directions_comments_strings_and_crate_roots_pass() {
    let usage = format!("use crate::foundation;\nuse crate::usage::decide;\n// crate::fleet::records::read_record\n{USAGE}\npub const NOTE: &str = \"crate::fleet::registry::root_here\";\n");
    let (lines, code) = lane(STRUCTURE, &usage);
    assert_eq!(code, 0, "{lines:?}");
    assert_eq!(lines, ["layers: no new reference breaks structure.toml's module layers or crate DAG"]);
}

#[test]
fn the_crate_dag_alone_refuses_when_the_module_layers_allow() {
    let loose = STRUCTURE.replacen("name = \"usage\"\npaths = [\"crates/north-core/src/usage/**\"]\nmay_use = []", "name = \"usage\"\npaths = [\"crates/north-core/src/usage/**\"]\nmay_use = [\"fleet\"]", 1);
    assert_ne!(loose, STRUCTURE);
    let (lines, code) = lane(&loose, &format!("use crate::fleet::records;\n{USAGE}"));
    assert_eq!(code, 1, "{lines:?}");
    assert!(!refused(&lines, "✗ layer "), "{lines:?}");
    assert!(refused(&lines, "✗ crate-dag crates/north-core/src/usage/mod.rs:1 new reference -> crates/north-core/src/fleet/records.rs: planned crate north-pace may not use north-fleet (its may_use: nothing; plan docs/rationale/crate-split.md#next-cuts)"), "{lines:?}");
}

#[test]
fn a_crate_dag_cycle_or_unknown_crate_is_an_error() {
    let cycle = STRUCTURE.replacen("name = \"north-pace\"\npaths = [\"crates/*/src/usage/**\"]\nmay_use = []", "name = \"north-pace\"\npaths = [\"crates/*/src/usage/**\"]\nmay_use = [\"north-fleet\"]", 1);
    assert_ne!(cycle, STRUCTURE);
    assert_eq!(Rules::parse(&cycle).err().unwrap(), "structure.toml: crate_dag cycle through north-pace");
    let unknown = STRUCTURE.replacen("may_use = [\"north-pace\"]", "may_use = [\"north-moon\"]", 1);
    assert_eq!(Rules::parse(&unknown).err().unwrap(), "structure.toml: crate_dag.crate north-fleet may_use unknown north-moon");
}

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)).unwrap()
}

#[test]
fn structure_toml_names_the_c12_plan_crates_and_their_dependencies() {
    let rules = Rules::parse(&repo_file("structure.toml")).unwrap();
    let plan: BTreeMap<String, BTreeSet<String>> = repo_file("harness/split/plans/c12.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next().unwrap().to_owned();
            let uses = fields.next().unwrap();
            (name, uses.split(',').filter(|dependency| *dependency != "-").map(str::to_owned).collect())
        })
        .collect();
    assert_eq!(plan.len(), 15);
    assert_eq!(rules.crate_plan(), plan);
}

#[test]
fn the_repository_rules_refuse_crate_fleet_in_usage() {
    let rules = Rules::parse(&repo_file("structure.toml")).unwrap();
    let names: Vec<String> = rules.breaches("crates/north-core/src/usage/shell/fleet_records.rs", "crates/north-core/src/fleet/records.rs").into_iter().map(|breach| format!("{:?} {} -> {}", breach.rule, breach.from, breach.to)).collect();
    assert_eq!(names, ["Layer usage -> fleet", "CrateDag north-pace -> north-fleet"]);
    assert!(rules.breaches("crates/north-core/src/fleet/handover.rs", "crates/north-core/src/usage/decide/mod.rs").iter().all(|breach| breach.from != "north-fleet"));
    assert!(rules.breaches("crates/north-core/src/usage/mod.rs", "crates/north-core/src/lib.rs").is_empty());
    assert!(rules.breaches("crates/north-dispatch/src/pacing/placement/hosts.rs", "crates/north-guard/src/guard/spawn_capacity.rs").iter().any(|breach| breach.from == "north-dispatch" && breach.to == "north-guard"));
}

#[test]
fn a_facade_module_may_re_export_what_depends_on_it() {
    let facade = STRUCTURE.replacen("[crate_dag]", "[[module]]\nname = \"roots\"\npaths = [\"crates/*/src/lib.rs\"]\nfacade = true\nmay_use = [\"usage\"]\n\n[crate_dag]", 1).replacen("may_use = []", "may_use = [\"roots\"]", 1);
    let rules = Rules::parse(&facade).unwrap();
    assert!(rules.breaches("crates/north-core/src/lib.rs", "crates/north-core/src/fleet/mod.rs").is_empty());
    assert!(rules.breaches("crates/north-core/src/usage/mod.rs", "crates/north-core/src/lib.rs").is_empty());
    let plain = facade.replace("facade = true\n", "");
    let rules = Rules::parse(&plain).unwrap();
    assert_eq!(rules.breaches("crates/north-core/src/lib.rs", "crates/north-core/src/fleet/mod.rs").len(), 1);
}
