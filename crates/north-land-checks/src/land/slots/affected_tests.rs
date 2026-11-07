use super::*;

fn metadata() -> String {
    let package = |name: &str, directory: &str, dependencies: &[&str]| {
        let manifest = if directory.is_empty() { "/w/Cargo.toml".to_owned() } else { format!("/w/{directory}/Cargo.toml") };
        let dependencies: Vec<_> = dependencies.iter().map(|dependency| serde_json::json!({ "name": dependency, "path": format!("/w/crates/{dependency}") })).chain([serde_json::json!({ "name": "serde", "path": null })]).collect();
        let targets = if directory.is_empty() { serde_json::json!([{ "name": name, "kind": ["bin"] }]) } else { serde_json::json!([{ "name": name, "kind": ["lib"] }, { "name": format!("{name}-cli"), "kind": ["bin"] }]) };
        serde_json::json!({ "name": name, "id": format!("{name}-id"), "manifest_path": manifest, "dependencies": dependencies, "targets": targets })
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

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn packages(affected: &[&str], changed: &[&str]) -> Scope {
    Scope::Packages { affected: names(affected), changed: names(changed) }
}

#[test]
fn a_leaf_crate_selects_itself_and_its_dependents_only() {
    assert_eq!(scope(&["crates/landing/src/lib.rs"]), packages(&["app", "landing"], &["landing"]));
    assert_eq!((scope(&["crates/landing/src/lib.rs"]).arguments(), scope(&["crates/landing/src/lib.rs"]).changed_arguments()), ("-p app -p landing".to_owned(), "-p landing".to_owned()));
    assert_eq!(scope(&["crates/core/Cargo.toml", "docs/x.md"]), packages(&["app", "core"], &["core"]));
}

#[test]
fn an_embedded_file_selects_the_packages_whose_dep_info_lists_it() {
    let core = "/w/target/debug/deps/core-1a.d: crates/core/src/lib.rs crates/core/src/../../../harness/defaults.toml\n\ncrates/core/src/lib.rs:\ncrates/core/src/../../../harness/defaults.toml:\n\n# env-dep:CARGO_MANIFEST_DIR=/w/crates/core\n";
    let landing = "/w/target/debug/deps/landing_cli-2b.d: crates/landing/src/main.rs /w/harness/defaults.toml /w/docs/feedback-loop.md\n\ncrates/landing/src/main.rs:\n/w/harness/defaults.toml:\n/w/docs/feedback-loop.md:\n";
    let external = "/w/target/debug/deps/serde-3c.d: /registry/serde/src/lib.rs /w/harness/model-catalog.toml\n\n/registry/serde/src/lib.rs:\n/w/harness/model-catalog.toml:\n";
    let dep_info = [("core-1a", core), ("landing_cli-2b", landing), ("serde-3c", external)];
    assert_eq!(embedded_scope(&["harness/defaults.toml"], &dep_info), packages(&["app", "core", "landing"], &["core", "landing"]));
    assert_eq!(embedded_scope(&["harness/defaults.toml"], &dep_info[..1]), packages(&["app", "core"], &["core"]));
    assert_eq!(embedded_scope(&["docs/feedback-loop.md"], &dep_info), packages(&["app", "landing"], &["landing"]));
    assert_eq!(embedded_scope(&["docs/feedback-loop.md", "crates/core/src/x.rs"], &dep_info), packages(&["app", "core", "landing"], &["core", "landing"]));
    assert_eq!(embedded_scope(&["docs/feedback-loop.md", "Cargo.lock"], &dep_info), Scope::Workspace);
    assert_eq!(embedded_scope(&["harness/model-catalog.toml"], &dep_info), Scope::Workspace);
}

