use super::file_skill::{Roots, decide, rules_len};
use crate::hook::{HookInput, Verdict};
use serde_json::json;
use std::path::{Path, PathBuf};

struct Fixture {
    dir: tempfile::TempDir,
    roots: Roots,
}

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let rust = skills.join("rust-development");
    std::fs::create_dir_all(rust.join("references")).unwrap();
    for file in ["SKILL.md", "references/compile-times.md"] {
        std::fs::copy(repo("agent-machinery/skills/rust-development").join(file), rust.join(file)).unwrap();
    }
    for skill in ["effect-development", "nix-development"] {
        std::fs::create_dir_all(skills.join(skill)).unwrap();
        std::fs::write(skills.join(skill).join("SKILL.md"), format!("---\nname: {skill}\n---\n\n# {skill} rules\n")).unwrap();
    }
    let roots = Roots { skills, seen: dir.path().join("seen") };
    Fixture { dir, roots }
}

fn edit(session: &str, tool: &str, path: &Path, extra: serde_json::Value) -> HookInput {
    let mut tool_input = json!({"file_path": path});
    tool_input.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let payload = json!({"session_id": session, "hook_event_name": "PreToolUse", "tool_name": tool, "cwd": "/tmp", "tool_input": tool_input});
    HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap()
}

fn advice(verdict: Verdict) -> String {
    match verdict {
        Verdict::Advise(text) => text,
        other => panic!("expected advice, got {other:?}"),
    }
}

#[test]
fn the_table_names_every_language_skill() {
    assert_eq!(rules_len(), 3);
}

#[test]
fn the_first_rust_edit_of_a_session_gets_the_skill_once_and_later_edits_do_not() {
    let f = fixture();
    let crate_dir = f.dir.path().join("proj");
    std::fs::create_dir_all(&crate_dir).unwrap();
    std::fs::write(crate_dir.join("Cargo.toml"), "[package]\nname = \"p\"\n\n[profile.dev]\ndebug = 0\n").unwrap();
    let first = advice(decide(&edit("s1", "Edit", &crate_dir.join("main.rs"), json!({})), &f.roots));
    assert!(first.contains("# Rust development"), "{first}");
    assert!(!first.contains("grounded:"), "frontmatter must be stripped: {first}");
    let pointer = format!("{}/rust-development/references/compile-times.md", f.roots.skills.display());
    assert!(first.contains(&pointer), "{first}");
    assert_eq!(decide(&edit("s1", "Edit", &crate_dir.join("lib.rs"), json!({})), &f.roots), Verdict::Allow);
    assert_eq!(decide(&edit("s1", "Write", &crate_dir.join("Cargo.toml"), json!({"content": "[package]\n[profile.dev]\n"})), &f.roots), Verdict::Allow);
    assert!(advice(decide(&edit("s2", "Edit", &crate_dir.join("main.rs"), json!({})), &f.roots)).contains("# Rust development"));
}

#[test]
fn a_ts_edit_gets_effect_only_where_effect_is_installed_and_nix_gets_its_skill() {
    let f = fixture();
    let with = f.dir.path().join("with");
    let without = f.dir.path().join("without");
    for dir in [&with, &without] {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join(".git"), "").unwrap();
    }
    std::fs::write(with.join("package.json"), r#"{"dependencies":{"effect":"^3"}}"#).unwrap();
    std::fs::write(without.join("package.json"), r#"{"dependencies":{"left-pad":"1"}}"#).unwrap();
    assert_eq!(decide(&edit("s", "Edit", &without.join("src/a.ts"), json!({})), &f.roots), Verdict::Allow);
    assert!(advice(decide(&edit("s", "Edit", &with.join("src/a.ts"), json!({})), &f.roots)).contains("# effect-development rules"));
    assert_eq!(decide(&edit("s", "Edit", &with.join("src/b.ts"), json!({})), &f.roots), Verdict::Allow);
    assert!(advice(decide(&edit("s", "Write", &with.join("flake.nix"), json!({})), &f.roots)).contains("# nix-development rules"));
}

#[test]
fn a_cargo_workspace_without_a_dev_profile_is_flagged_once_with_the_reference_lines() {
    let f = fixture();
    let root = f.dir.path().join("window");
    std::fs::create_dir_all(root.join("crates/a/src")).unwrap();
    let manifest = "[workspace]\nmembers = [\"crates/a\"]\n";
    let first = advice(decide(&edit("s", "Write", &root.join("Cargo.toml"), json!({"content": manifest})), &f.roots));
    assert!(first.contains("# Rust development"), "{first}");
    assert!(first.contains("no [profile.dev] section"), "{first}");
    assert!(first.contains("line-tables-only") && first.contains("Optimize dependencies only"), "{first}");
    assert!(!first.contains("`incremental`"), "{first}");
    std::fs::write(root.join("Cargo.toml"), manifest).unwrap();
    assert_eq!(decide(&edit("s", "Write", &root.join("crates/a/src/lib.rs"), json!({})), &f.roots), Verdict::Allow);
    let other = advice(decide(&edit("t", "Write", &root.join("crates/a/src/lib.rs"), json!({})), &f.roots));
    assert!(other.contains("no [profile.dev] section"), "{other}");
    std::fs::write(root.join("Cargo.toml"), format!("{manifest}\n[profile.dev]\ndebug = \"line-tables-only\"\n")).unwrap();
    let covered = advice(decide(&edit("u", "Write", &root.join("crates/a/src/lib.rs"), json!({})), &f.roots));
    assert!(!covered.contains("profile.dev"), "{covered}");
}

#[test]
fn a_missing_skill_directory_allows_and_a_patch_that_adds_the_dev_profile_is_not_flagged() {
    let f = fixture();
    let root = f.dir.path().join("crate");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    let empty = Roots { skills: f.dir.path().join("none"), seen: f.dir.path().join("seen-none") };
    assert_eq!(decide(&edit("s", "Write", &root.join("Cargo.toml"), json!({"content": "[package]\n"})), &empty), Verdict::Allow);
    let patch = format!("*** Begin Patch\n*** Update File: {}\n@@\n+[profile.dev]\n+debug = 0\n*** End Patch", root.join("Cargo.toml").display());
    let call = json!({"session_id": "p", "hook_event_name": "PreToolUse", "tool_name": "apply_patch", "cwd": "/tmp", "tool_input": {"command": patch}});
    let input = HookInput::parse(&serde_json::to_vec(&call).unwrap()).unwrap();
    let text = advice(decide(&input, &f.roots));
    assert!(!text.contains("profile.dev"), "{text}");
}

#[test]
fn a_patch_that_adds_the_dev_profile_to_one_manifest_still_flags_another_that_lacks_it() {
    let f = fixture();
    let covered = f.dir.path().join("covered");
    let bare = f.dir.path().join("bare");
    for dir in [&covered, &bare] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    }
    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n+[profile.dev]\n+debug = 0\n*** Update File: {}\n@@\n+edition = \"2024\"\n*** End Patch",
        covered.join("Cargo.toml").display(),
        bare.join("Cargo.toml").display()
    );
    let call = json!({"session_id": "m", "hook_event_name": "PreToolUse", "tool_name": "apply_patch", "cwd": "/tmp", "tool_input": {"command": patch}});
    let text = advice(decide(&HookInput::parse(&serde_json::to_vec(&call).unwrap()).unwrap(), &f.roots));
    assert_eq!(text.matches("no [profile.dev] section").count(), 1, "{text}");
}

#[test]
fn every_relative_link_in_an_installed_skill_resolves_from_its_installed_path() {
    let roots = ["agent-machinery/skills", "harness/skills"].map(repo);
    let source = |id: &str| roots.iter().map(|root| root.join(id)).find(|dir| dir.join("SKILL.md").is_file());
    let link = regex::Regex::new(r"\]\(([^)#\s]+)").unwrap();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for skill in roots.iter().flat_map(|root| std::fs::read_dir(root).unwrap()).map(|entry| entry.unwrap().path()) {
        let id = skill.file_name().unwrap().to_str().unwrap().to_owned();
        let mut pending = vec![skill.clone()];
        while let Some(dir) = pending.pop() {
            for path in std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().path()) {
                match path.is_dir() {
                    true => pending.push(path),
                    false if path.extension().is_some_and(|e| e == "md") => files.push((id.clone(), path.strip_prefix(&skill).unwrap().to_owned())),
                    false => {}
                }
            }
        }
    }
    assert!(files.iter().any(|(id, _)| id == "rust-development"));
    let mut broken = Vec::new();
    for (id, file) in &files {
        let text = std::fs::read_to_string(source(id).unwrap().join(file)).unwrap();
        let prose = text.split("```").step_by(2).collect::<Vec<_>>().join("\n");
        let from = Path::new("shared").join(id).join(file);
        for target in link.captures_iter(&prose).map(|c| c[1].to_owned()).filter(|t| !t.contains("://") && !t.starts_with('/') && !t.starts_with("mailto:")) {
            let mut resolved = PathBuf::new();
            for part in from.parent().unwrap().join(&target).components() {
                match part {
                    std::path::Component::ParentDir => drop(resolved.pop()),
                    other => resolved.push(other),
                }
            }
            let mut parts = resolved.components();
            let inside = parts.next().is_some_and(|c| c.as_os_str() == "shared");
            let target_id = parts.next().and_then(|c| c.as_os_str().to_str()).unwrap_or("").to_owned();
            if !(inside && source(&target_id).is_some_and(|dir| dir.join(parts.as_path()).is_file())) {
                broken.push(format!("{}: {target}", from.display()));
            }
        }
    }
    assert!(broken.is_empty(), "links that do not resolve in the installed skills:\n{}", broken.join("\n"));
}
