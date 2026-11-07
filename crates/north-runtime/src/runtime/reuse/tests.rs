use north_base::build_inputs::{run_time_data, rust_build_input};
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
    assert!(embedded > 12, "found only {embedded} embedded files; the scan stopped finding include macros");
}


#[test]
fn no_source_or_test_embeds_a_table_the_runtime_reads_from_its_tree() {
    let root = normal(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let include = Regex::new(r#"include(?:_str|_bytes)?!\(\s*"([^"]+)""#).unwrap();
    let mut files = Vec::new();
    sources(&root.join("crates"), &mut files);
    sources(&root.join("src"), &mut files);
    sources(&root.join("tests"), &mut files);
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for found in include.captures_iter(&text) {
            let resolved = normal(&file.parent().unwrap().join(&found[1]));
            let relative = resolved.strip_prefix(&root).map(|relative| relative.to_string_lossy().into_owned()).unwrap_or_default();
            assert!(!run_time_data(&relative), "{} embeds {relative}, which north reads at run time from its pinned tree", file.display());
        }
    }
}


#[test]
fn a_lock_that_only_adds_a_package_still_lets_the_live_runtime_run_an_unchanged_checker() {
    let checkout = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").current_dir(checkout.path()).args(args).status().unwrap().success());
    let commit = |lock: &str| {
        fs::write(checkout.path().join("Cargo.lock"), lock).unwrap();
        git(&["add", "Cargo.lock"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "lock"]);
        String::from_utf8(std::process::Command::new("git").current_dir(checkout.path()).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    let serde = "[[package]]\nname = \"serde\"\nversion = \"1.0.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aa\"\n";
    let live = commit(&format!("version = 4\n\n[[package]]\nname = \"north\"\nversion = \"0.1.0\"\ndependencies = [\n \"serde\",\n]\n\n{serde}"));
    let added = commit(&format!("version = 4\n\n[[package]]\nname = \"north\"\nversion = \"0.1.0\"\ndependencies = [\n \"north-frontier\",\n \"serde\",\n]\n\n[[package]]\nname = \"north-frontier\"\nversion = \"0.1.0\"\n\n{serde}"));
    let bumped = commit(&format!("version = 4\n\n{}", serde.replace("1.0.1", "1.0.2").replace("aa", "bb")));
    assert!(super::keeps_every_locked_package(checkout.path(), &live, &added), "laptop safe-push of the north-frontier lane built north from the tree in depbox, which reaches no build host (north#671)");
    assert!(!super::keeps_every_locked_package(checkout.path(), &live, &bumped), "a dependency the live runtime was built with moved, so it no longer covers the checker");
}