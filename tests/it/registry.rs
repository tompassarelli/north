use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn tests_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn declared(file: &Path, reached: &mut BTreeSet<PathBuf>) {
    if !reached.insert(file.to_path_buf()) {
        return;
    }
    let text = std::fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    let parent = file.parent().unwrap();
    let stem = file.file_stem().unwrap().to_str().unwrap();
    let children = if matches!(stem, "main" | "mod") { parent.to_path_buf() } else { parent.join(stem) };
    let mut path_attr = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#[path = \"") {
            path_attr = rest.strip_suffix("\"]").map(str::to_owned);
            continue;
        }
        let name = line.strip_prefix("pub ").unwrap_or(line).strip_prefix("mod ").and_then(|rest| rest.strip_suffix(';'));
        if let Some(name) = name {
            let target = match path_attr.take() {
                Some(path) => parent.join(path),
                None if children.join(format!("{name}.rs")).exists() => children.join(format!("{name}.rs")),
                None => children.join(name).join("mod.rs"),
            };
            declared(&target, reached);
        } else if !line.starts_with("#[") {
            path_attr = None;
        }
    }
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

pub fn unregistered(root: &Path) -> Vec<String> {
    let mut reached = BTreeSet::new();
    declared(&root.join("it/main.rs"), &mut reached);
    let mut found = Vec::new();
    sources(root, &mut found);
    found.sort();
    found
        .into_iter()
        .filter(|path| !reached.contains(path))
        .map(|path| {
            let relative = path.strip_prefix(root).unwrap();
            let name = path.file_stem().unwrap().to_str().unwrap();
            match relative.parent().and_then(Path::to_str) {
                Some("") => format!("tests/{} builds a test binary of its own: move it to tests/it/{name}.rs and add `mod {name};` to tests/it/main.rs", relative.display()),
                Some("it") => format!("tests/{} never runs: add `mod {name};` to tests/it/main.rs", relative.display()),
                _ => format!("tests/{} never runs: declare it with `mod {name};` in its parent module", relative.display()),
            }
        })
        .collect()
}

#[test]
fn every_test_file_is_compiled_into_this_binary() {
    let missing = unregistered(&tests_root());
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
