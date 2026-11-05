use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TOOLS: &[&str] = &[
    "bash", "cargo", "cat", "dirname", "fd", "flock", "git", "grep", "head", "jq", "mkdir", "mktemp", "readlink", "rm", "rsync", "sed", "yq", "bun",
];

fn locate(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(tool)).find(|path| path.is_file())
}

fn path_without(missing: &str, directory: &Path) -> String {
    for tool in TOOLS.iter().filter(|tool| **tool != missing) {
        if let Some(found) = locate(tool) {
            let _ = symlink(found, directory.join(tool));
        }
    }
    directory.display().to_string()
}

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/scripts").join(name)
}

fn run_without(name: &str, missing: &str, arguments: &[&str], environment: &[(&str, &str)]) -> Output {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let work = root.path().join("work");
    fs::create_dir(&work).unwrap();
    Command::new(locate("bash").unwrap())
        .arg(script(name))
        .args(arguments)
        .current_dir(&work)
        .env_clear()
        .env("PATH", path_without(missing, &bin))
        .env("HOME", root.path())
        .envs(environment.iter().copied())
        .output()
        .unwrap()
}

fn assert_names(output: &Output, tool: &str) {
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(!output.status.success(), "{tool} absent but the script passed: {text}");
    assert!(text.contains(tool), "the failure does not name {tool}: {text}");
}

#[test]
fn canary_fails_closed_without_each_tool() {
    for tool in ["git", "yq", "fd"] {
        assert_names(&run_without("canary.sh", tool, &["any"], &[]), tool);
    }
}

#[test]
fn cargo_tests_fails_closed_without_each_tool() {
    for tool in ["cargo", "jq"] {
        assert_names(&run_without("cargo-tests.sh", tool, &[], &[]), tool);
    }
}

#[test]
fn mutant_tests_fails_closed_without_each_tool() {
    let cache = tempfile::tempdir().unwrap();
    let cache = cache.path().to_str().unwrap();
    for tool in ["cargo", "flock", "rsync"] {
        let output = run_without("mutant-tests.sh", tool, &[], &[("MUTANT_TESTS", "kills_it"), ("MUTANT_TREES", ""), ("XDG_CACHE_HOME", cache)]);
        assert_names(&output, tool);
    }
    let output = run_without("mutant-tests.sh", "bun", &[], &[("MUTANT_TESTS", "a claimed bun test"), ("MUTANT_TREES", ""), ("XDG_CACHE_HOME", cache)]);
    assert_names(&output, "bun");
}

#[test]
fn check_owners_fails_closed_without_head_north() {
    assert_names(&run_without("check-owners.sh", "head-north", &[], &[]), "head-north");
}

#[test]
fn citation_check_fails_closed_without_head_north() {
    assert_names(&run_without("citation-check.sh", "head-north", &[], &[]), "head-north");
}
