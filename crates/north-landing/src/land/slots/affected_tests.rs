use super::*;

fn metadata() -> String {
    let package = |name: &str, directory: &str, dependencies: &[&str]| {
        let manifest = if directory.is_empty() { "/w/Cargo.toml".to_owned() } else { format!("/w/{directory}/Cargo.toml") };
        let dependencies: Vec<_> = dependencies.iter().map(|dependency| serde_json::json!({ "name": dependency, "path": format!("/w/crates/{dependency}") })).chain([serde_json::json!({ "name": "serde", "path": null })]).collect();
        serde_json::json!({ "name": name, "id": format!("{name}-id"), "manifest_path": manifest, "dependencies": dependencies, "targets": [{ "name": name }, { "name": format!("{name}-cli") }] })
    };
    serde_json::json!({
        "packages": [package("base", "crates/base", &[]), package("core", "crates/core", &["base"]), package("landing", "crates/landing", &["base"]), package("app", "", &["core", "landing"])],
        "workspace_members": ["base-id", "core-id", "landing-id", "app-id"],
        "workspace_root": "/w",
    })
    .to_string()
}

fn scope(files: &[&str]) -> Scope {
    embedded_scope(files, &[])
}

fn embedded_scope(files: &[&str], dep_info: &[(&str, &str)]) -> Scope {
    let dep_info: Vec<_> = dep_info.iter().map(|(stem, contents)| DepInfo { stem: (*stem).to_owned(), contents: (*contents).to_owned() }).collect();
    select(&metadata(), &files.iter().map(|file| (*file).to_owned()).collect::<Vec<_>>(), &dep_info).unwrap()
}

fn packages(names: &[&str]) -> Scope {
    Scope::Packages(names.iter().map(|name| (*name).to_owned()).collect())
}

#[test]
fn a_leaf_crate_selects_itself_and_its_dependents_only() {
    assert_eq!(scope(&["crates/landing/src/lib.rs"]), packages(&["app", "landing"]));
    assert_eq!(scope(&["crates/core/Cargo.toml", "docs/x.md"]), packages(&["app", "core"]));
    assert_eq!(scope(&["src/main.rs", "tests/it/x.rs"]), packages(&["app"]));
}

#[test]
fn a_crate_every_member_depends_on_selects_the_workspace() {
    assert_eq!(scope(&["crates/base/src/lib.rs"]), Scope::Workspace);
}

#[test]
fn a_workspace_manifest_lock_or_toolchain_change_selects_the_workspace() {
    for file in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "flake.lock", "harness/model-catalog.toml"] {
        assert_eq!(scope(&["crates/landing/src/lib.rs", file]), Scope::Workspace, "{file}");
    }
}

#[test]
fn no_rust_change_selects_the_workspace() {
    assert_eq!(scope(&["docs/x.md"]), Scope::Workspace);
}

#[test]
fn packages_become_cargo_arguments_and_a_cpu_count() {
    let narrow = packages(&["app", "landing"]);
    assert_eq!(narrow.arguments(), "-p app -p landing");
    assert_eq!(narrow.cpus(), Some(4));
    assert_eq!(packages(&["a", "b", "c"]).cpus(), Some(6));
    assert_eq!(Scope::Workspace.arguments(), "--workspace");
    assert_eq!(Scope::Workspace.cpus(), None);
}

#[test]
fn an_embedded_file_selects_the_packages_whose_dep_info_lists_it() {
    let core = "/w/target/debug/deps/core-1a.d: crates/core/src/lib.rs crates/core/src/../../../harness/defaults.toml\n\ncrates/core/src/lib.rs:\ncrates/core/src/../../../harness/defaults.toml:\n\n# env-dep:CARGO_MANIFEST_DIR=/w/crates/core\n";
    let landing = "/w/target/debug/deps/landing_cli-2b.d: crates/landing/src/main.rs /w/harness/defaults.toml /w/docs/feedback-loop.md\n\ncrates/landing/src/main.rs:\n/w/harness/defaults.toml:\n/w/docs/feedback-loop.md:\n";
    let external = "/w/target/debug/deps/serde-3c.d: /registry/serde/src/lib.rs /w/harness/model-catalog.toml\n\n/registry/serde/src/lib.rs:\n/w/harness/model-catalog.toml:\n";
    let dep_info = [("core-1a", core), ("landing_cli-2b", landing), ("serde-3c", external)];
    assert_eq!(embedded_scope(&["harness/defaults.toml"], &dep_info), packages(&["app", "core", "landing"]));
    assert_eq!(embedded_scope(&["harness/defaults.toml"], &dep_info[..1]), packages(&["app", "core"]));
    assert_eq!(embedded_scope(&["docs/feedback-loop.md"], &dep_info), packages(&["app", "landing"]));
    assert_eq!(embedded_scope(&["docs/feedback-loop.md", "crates/core/src/x.rs"], &dep_info), packages(&["app", "core", "landing"]));
    assert_eq!(embedded_scope(&["docs/feedback-loop.md", "Cargo.lock"], &dep_info), Scope::Workspace);
    assert_eq!(embedded_scope(&["harness/model-catalog.toml"], &dep_info), Scope::Workspace);
}

#[test]
fn an_embedded_file_without_dep_info_selects_the_workspace() {
    assert_eq!(scope(&["harness/defaults.toml"]), Scope::Workspace);
    assert_eq!(scope(&["docs/feedback-loop.md"]), Scope::Workspace);
}

#[test]
fn every_affected_include_target_is_a_rust_build_input() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap();
    let mut sources = Vec::new();
    for directory in ["crates", "src", "tests", "examples", "benches"] {
        rust_sources(&root.join(directory), &mut sources);
    }
    let mut targets = Vec::new();
    for source in &sources {
        let text = std::fs::read_to_string(source).unwrap();
        for literal in include_targets(&text) {
            let target = workspace_path(&root, &source.parent().unwrap().join(literal).to_string_lossy()).unwrap();
            targets.push(target);
        }
    }
    assert!(targets.iter().any(|target| target == "harness/defaults.toml"), "{targets:?}");
    let missing: Vec<_> = targets.iter().filter(|target| !rust_build_input(target)).collect();
    assert!(missing.is_empty(), "embedded files missing from north-base build_inputs EMBEDDED: {missing:?}");
}

fn rust_sources(directory: &Path, sources: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for path in entries.map(|entry| entry.unwrap().path()) {
        if path.is_dir() {
            rust_sources(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
}

fn include_targets(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    for macro_name in ["include_str!", "include_bytes!"] {
        for (start, _) in text.match_indices(macro_name) {
            let rest = text[start + macro_name.len()..].trim_start();
            let Some(rest) = rest.strip_prefix('(') else { continue };
            let Some(rest) = rest.trim_start().strip_prefix('"') else { continue };
            if let Some(end) = rest.find('"') {
                targets.push(&rest[..end]);
            }
        }
    }
    targets
}
