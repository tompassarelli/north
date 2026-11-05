use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::guides::{Entry, notebook_markdown, staleness};
use super::package_check::check_package;

type Mutation = fn(&mut Value, &Path);

fn catalog() -> Value {
    json!({
        "$schema": "urn:agent-machinery:schema:catalog:v1",
        "schema": "agent-machinery.catalog/v1",
        "package": {"license": "MIT OR Apache-2.0"},
        "units": [
            {"id": "demo", "kind": "skill", "source": "skills/demo/SKILL.md"},
            {"id": "core", "kind": "module", "source": "doctrine.md", "members": ["demo"]}
        ],
        "assets": [
            {"id": "doctrine", "type": "instructions", "path": "doctrine.md"},
            {"id": "catalog-schema", "type": "catalog", "path": "catalog.schema.json"}
        ],
        "contracts": [
            {"id": "demo-v1", "schema": "contracts/demo.schema.json", "schemaScope": "structural", "validator": "validateContract", "fixtures": "contracts/demo.fixtures.json"}
        ]
    })
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn package(mutation: Mutation) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "skills/demo/SKILL.md", "---\nname: demo\ndescription: Demo.\n---\nBody.\n");
    write(root, "doctrine.md", "Doctrine.\n");
    write(root, "catalog.schema.json", r#"{"$id": "urn:agent-machinery:schema:catalog:v1"}"#);
    write(root, "contracts/demo.schema.json", r#"{"$id": "urn:agent-machinery:schema:demo:v1"}"#);
    write(root, "contracts/demo.fixtures.json", "{}");
    let mut value = catalog();
    mutation(&mut value, root);
    write(root, "catalog.json", &value.to_string());
    dir
}

fn cases() -> Vec<(&'static str, Mutation)> {
    vec![
        ("export catalog must retain the closed six-key package shape", |c, _| c["extra"] = json!(1)),
        ("unexpected catalog schema", |c, _| c["schema"] = json!("agent-machinery.catalog/v2")),
        ("unexpected package license", |c, _| c["package"]["license"] = json!("MIT")),
        ("export catalog contains activation field owner", |c, _| c["package"]["owner"] = json!("someone")),
        ("duplicate unit id demo", |c, _| c["units"][1]["id"] = json!("demo")),
        ("invalid unit kind for demo", |c, _| c["units"][0]["kind"] = json!("hook")),
        ("unit demo source must be package-relative", |c, _| c["units"][0]["source"] = json!("/etc/passwd")),
        ("unit demo source escapes package", |c, _| c["units"][0]["source"] = json!("skills/../../outside.md")),
        ("missing unit source skills/absent/SKILL.md", |c, _| c["units"][0]["source"] = json!("skills/absent/SKILL.md")),
        ("module core has no members", |c, _| c["units"][1]["members"] = json!([])),
        ("skill demo must not declare members", |c, _| c["units"][0]["members"] = json!(["core"])),
        ("unit demo source name mismatch", |c, root| {
            write(root, "skills/other/SKILL.md", "---\nname: other\n---\n");
            c["units"][0]["source"] = json!("skills/other/SKILL.md");
        }),
        ("skills/demo/SKILL.md has no frontmatter", |_, root| write(root, "skills/demo/SKILL.md", "name: demo\n")),
        ("skills/demo/SKILL.md has no name", |_, root| write(root, "skills/demo/SKILL.md", "---\ndescription: Demo.\n---\n")),
        ("module core has unknown member absent", |c, _| c["units"][1]["members"] = json!(["demo", "absent"])),
        ("duplicate asset id doctrine", |c, _| c["assets"][1]["id"] = json!("doctrine")),
        ("invalid asset type for doctrine", |c, _| c["assets"][0]["type"] = json!("hook")),
        ("missing asset absent.md", |c, _| c["assets"][0]["path"] = json!("absent.md")),
        ("schema catalog.schema.json has no stable versioned package ID", |_, root| {
            write(root, "catalog.schema.json", r#"{"$id": "urn:agent-machinery:schema:catalog:v0"}"#)
        }),
        ("duplicate schema id urn:agent-machinery:schema:catalog:v1", |_, root| {
            write(root, "contracts/demo.schema.json", r#"{"$id": "urn:agent-machinery:schema:catalog:v1"}"#)
        }),
        ("catalog $schema is absent from declared schema assets", |c, _| c["$schema"] = json!("urn:agent-machinery:schema:absent:v1")),
        ("contract demo-v1 must classify its raw schema as structural", |c, _| c["contracts"][0]["schemaScope"] = json!("semantic")),
        ("contract demo-v1 must use the composed validator", |c, _| c["contracts"][0]["validator"] = json!("ajv")),
        ("missing fixtures for demo-v1", |c, _| c["contracts"][0]["fixtures"] = json!("contracts/absent.json")),
        ("forbidden source boundary directory: hooks/run.md", |_, root| write(root, "hooks/run.md", "Run.\n")),
        ("provider interface metadata is outside package authority: skills/demo/agents/openai.yaml", |_, root| {
            write(root, "skills/demo/agents/openai.yaml", "name: demo\n")
        }),
        ("non-portable source marker in doctrine.md", |_, root| write(root, "doctrine.md", "See ~/notes.\n")),
        ("non-portable source marker in skills/demo/SKILL.md", |_, root| {
            write(root, "skills/demo/SKILL.md", "---\nname: demo\n---\nRun on nixos.\n")
        }),
    ]
}

#[test]
fn a_well_formed_package_passes_and_counts_its_units() {
    assert_eq!(check_package(package(|_, _| ()).path()), Ok(2));
}

#[test]
fn the_shipped_agent_machinery_package_passes() {
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent-machinery");
    assert!(check_package(&shipped).is_ok(), "{:?}", check_package(&shipped));
}

#[test]
fn each_broken_guarantee_fails_with_its_own_message() {
    for (expected, mutation) in cases() {
        assert_eq!(check_package(package(mutation).path()), Err(expected.to_owned()), "case {expected}");
    }
}

#[test]
fn licence_files_large_files_and_dependency_trees_are_not_scanned_for_markers() {
    let dir = package(|_, root| {
        write(root, "NOTICE", "Built on NixOS under /home/.\n");
        write(root, "node_modules/dep/hooks/index.md", "mcp__tool\n");
        write(root, "assets/large.txt", &format!("{}/home/\n", "x".repeat(1_000_000)));
    });
    assert_eq!(check_package(dir.path()), Ok(2));
}

#[test]
fn guides_older_than_ninety_days_are_stale() {
    let entry = |out: &str, retrieved: &str| Entry { out: out.into(), url: String::new(), license: "MIT".into(), retrieved: retrieved.into() };
    let today = time::Date::from_calendar_date(2027, time::Month::January, 6).expect("date");
    let report = staleness(&[entry("fresh.md", "2026-10-08"), entry("old.md", "2026-10-07")], today).expect("staleness");
    assert_eq!(report.stale, 1);
    assert_eq!(report.lines[..2], ["  90 days  fresh.md".to_owned(), "  91 days  old.md".to_owned()]);
    assert!(staleness(&[entry("bad.md", "October")], today).is_err());
}

#[test]
fn notebooks_become_markdown_with_fenced_code_cells() {
    let notebook = r##"{"cells": [{"cell_type": "markdown", "source": ["# Title\n", "Text"]}, {"cell_type": "code", "source": "print(1)"}]}"##;
    assert_eq!(notebook_markdown(notebook).expect("notebook"), "# Title\nText\n\n```\nprint(1)\n```");
}

