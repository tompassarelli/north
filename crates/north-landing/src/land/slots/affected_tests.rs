use super::*;

fn metadata() -> String {
    let package = |name: &str, directory: &str, dependencies: &[&str]| {
        let manifest = if directory.is_empty() { "/w/Cargo.toml".to_owned() } else { format!("/w/{directory}/Cargo.toml") };
        let dependencies: Vec<_> = dependencies.iter().map(|dependency| serde_json::json!({ "name": dependency, "path": format!("/w/crates/{dependency}") })).chain([serde_json::json!({ "name": "serde", "path": null })]).collect();
        serde_json::json!({ "name": name, "id": format!("{name}-id"), "manifest_path": manifest, "dependencies": dependencies })
    };
    serde_json::json!({
        "packages": [package("base", "crates/base", &[]), package("core", "crates/core", &["base"]), package("landing", "crates/landing", &["base"]), package("app", "", &["core", "landing"])],
        "workspace_members": ["base-id", "core-id", "landing-id", "app-id"],
        "workspace_root": "/w",
    })
    .to_string()
}

fn scope(files: &[&str]) -> Scope {
    select(&metadata(), &files.iter().map(|file| (*file).to_owned()).collect::<Vec<_>>()).unwrap()
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
