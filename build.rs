//! Writes each generated registry from its per-unit source tree before the crate compiles, so a
//! lane that adds a command, a verb or a module never edits the generated file and its build still
//! sees the fresh one. `north docs ...` runs the same renderers, and the lander runs them again after
//! it picks a lane, which is the copy that lands.
use std::fs;
use std::path::Path;

fn write(root: &Path, source: &str, target: &str, render: fn(&Path) -> Result<String, String>) {
    let source = root.join(source);
    println!("cargo:rerun-if-changed={}", source.display());
    if !source.is_dir() {
        return;
    }
    let text = render(&source).unwrap_or_else(|error| panic!("{error}"));
    let target = root.join(target);
    if fs::read_to_string(&target).is_ok_and(|current| current == text) {
        return;
    }
    let temporary = target.with_extension(format!("tmp-{}", std::process::id()));
    if let Err(error) = fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, &target)) {
        let _ = fs::remove_file(&temporary);
        panic!("{}: {error}", target.display());
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    println!("cargo:rerun-if-changed=build.rs");
    write(root, "src/cli/reference", "src/cli/reference.txt", north_registries::reference);
    write(root, "src/cli/verbs", "src/cli/verbs.rs", north_registries::verbs);
    write(root, "structure", "structure.toml", north_registries::structure);
}
