use super::reuse;
use north_base::build_inputs::rust_build_input;
use crate::ops::runtime::layout::{self, BUILT_HERE, Layout, PROGRAMS};
use regex::Regex;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[test]
fn only_files_the_release_build_reads_count_as_rust_inputs() {
    for path in [
        "crates/north-ops/src/ops/runtime/update.rs",
        "crates/north-core/src/usage/fixtures/states.json",
        "src/main.rs",
        "src/cli/help.txt",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "flake.nix",
        "flake.lock",
        "bin/cargo",
        ".cargo/config.toml",
        "build.rs",
        ".gitattributes",
        "harness/.gitattributes",
        "harness/defaults.toml",
        "harness/model-catalog.toml",
        "harness/model-watch-sources.toml",
        "agent-machinery/file-skills.toml",
        "north-data/private-checks.toml",
        "north-data/expectations.toml",
        "bin/threads",
    ] {
        assert!(rust_build_input(path), "{path}");
    }
    for path in [
        "docs/features.md",
        "AGENTS.md",
        "harness/skills/machine-capacity/SKILL.md",
        "harness/hooks/guard.sh",
        "harness/lib/usage.py",
        "tests/runtime_update.rs",
        ".github/workflows/ci.yml",
        "agent-machinery/catalog.json",
        "srcs/readme.md",
    ] {
        assert!(!rust_build_input(path), "{path}");
    }
}

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

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, file: &str) -> String {
    let path = dir.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, file).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

fn live_runtime(state: &Path, live: &str, built_here: bool) -> Layout {
    let layout = Layout { state: state.to_path_buf() };
    let runtime = layout.runtime(live);
    fs::create_dir_all(&runtime).unwrap();
    for program in PROGRAMS {
        fs::write(runtime.join(program), program).unwrap();
    }
    if built_here {
        fs::write(runtime.join(BUILT_HERE), "").unwrap();
    }
    std::os::unix::fs::symlink(layout::link(live), layout.current()).unwrap();
    layout
}

#[test]
fn a_landing_with_no_rust_input_reuses_the_live_runtime_on_a_host_that_builds_only_its_own() {
    let repo = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    let live = commit(repo.path(), "crates/north-core/src/lib.rs");
    let target = commit(repo.path(), "docs/features.md");
    let layout = live_runtime(state.path(), &live, true);
    let wanted = layout.runtime(&target);
    assert!(!layout::usable(&wanted, true));
    assert!(reuse(&layout, repo.path(), &target, &wanted, true));
    assert!(layout::usable(&wanted, true));
    assert_eq!(fs::read_to_string(wanted.join("north")).unwrap(), "north");
    assert_eq!(fs::read_to_string(wanted.join(BUILT_HERE)).unwrap(), layout::short(&target));
}

#[test]
fn a_host_that_builds_only_its_own_never_reuses_a_live_runtime_it_did_not_build() {
    let repo = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    let live = commit(repo.path(), "crates/north-core/src/lib.rs");
    let target = commit(repo.path(), "docs/features.md");
    let layout = live_runtime(state.path(), &live, false);
    let wanted = layout.runtime(&target);
    assert!(!reuse(&layout, repo.path(), &target, &wanted, true));
    assert!(!wanted.exists());
    assert!(reuse(&layout, repo.path(), &target, &wanted, false));
    assert!(!wanted.join(BUILT_HERE).exists());
}

#[test]
fn a_landing_that_changes_a_rust_input_is_not_reused() {
    let repo = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    let live = commit(repo.path(), "docs/features.md");
    let target = commit(repo.path(), "crates/north-core/src/lib.rs");
    let layout = live_runtime(state.path(), &live, true);
    let wanted = layout.runtime(&target);
    assert!(!reuse(&layout, repo.path(), &target, &wanted, true));
}
