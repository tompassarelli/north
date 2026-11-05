use north_base::build_inputs::rust_build_input;
use regex::Regex;
use std::fs;
use std::path::{Component, Path, PathBuf};

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_none_or(|name| name != "target") {
                sources(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

fn test_only(file: &Path) -> bool {
    let name = file.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    name == "tests.rs" || name.ends_with("_tests.rs")
}

fn normal(path: &Path) -> PathBuf {
    path.components().fold(PathBuf::new(), |mut built, part| {
        match part {
            Component::ParentDir => drop(built.pop()),
            Component::CurDir => {}
            other => built.push(other),
        }
        built
    })
}

#[test]
fn every_file_a_source_embeds_at_compile_time_counts_as_a_rust_input() {
    let root = normal(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let include = Regex::new(r#"include(?:_str|_bytes)?!\(\s*"([^"]+)""#).unwrap();
    let mut files = Vec::new();
    sources(&root.join("crates"), &mut files);
    sources(&root.join("src"), &mut files);
    let mut embedded = 0;
    for file in files.into_iter().filter(|file| !test_only(file)) {
        let text = fs::read_to_string(&file).unwrap();
        for found in include.captures_iter(&text) {
            let resolved = normal(&file.parent().unwrap().join(&found[1]));
            let relative = resolved.strip_prefix(&root).unwrap_or_else(|_| panic!("{} embeds {} outside the repository", file.display(), &found[1]));
            let relative = relative.to_string_lossy();
            assert!(rust_build_input(&relative), "{} embeds {relative}, which north runtime update would not treat as a Rust build input", file.display());
            embedded += 1;
        }
    }
    assert!(embedded > 50, "found only {embedded} embedded files; the scan stopped finding include macros");
}

