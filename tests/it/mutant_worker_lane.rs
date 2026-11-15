use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::support::bin_exe;

const BROKER: &str = r#"case "$(pwd -P)/" in "LANE"/*) ;; *) echo "north worker-sandbox broker: refused: $(pwd -P) is in no lane (PROJECT/worktrees/NAME) and no main checkout (PROJECT/main) of this host's projects, and a request is served only from one; run it from your lane" >&2; exit 75 ;; esac"#;

const TEST_BINARY: &str = r#"case "$1" in --list) grep -q 'fn guards_first' src/lib_tests.rs && echo "guards_first: test"; grep -q 'fn idle_second' src/lib_tests.rs && echo "idle_second: test"; exit 0 ;; esac
[ -e src/silent ] && exit 3
grep -q 'f() -> u8 { 3 }' src/lib.rs && echo "test guards_first ... ok" || echo "test guards_first ... FAILED"
grep -q 'fn idle_second' src/lib_tests.rs && echo "test idle_second ... ok"
exit 0"#;

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn script(path: &Path, lane: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let guard = BROKER.replace("LANE", &lane.display().to_string());
    std::fs::write(path, format!("#!/bin/sh\n{guard}\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn north_with(home: &Path, directory: &Path, path: &str, arguments: &[&str], variables: &[(&str, &str)]) -> std::process::Output {
    Command::new(bin_exe!("CARGO_BIN_EXE_north"))
        .current_dir(directory)
        .args(arguments)
        .envs(variables.iter().copied())
        .env("PATH", path)
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn mutant_runs_judges_each_lane_tree_on_its_own_tests_and_records_a_fail_only_from_a_test_line() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().canonicalize().unwrap();
    let lane = home.join("code/north/worktrees/lane");
    let tools = home.join("tools");
    std::fs::create_dir_all(&lane).unwrap();
    git_in(&lane, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("lib_tests"), format!("#!/bin/sh\n{TEST_BINARY}\n")).unwrap();
    std::fs::set_permissions(tools.join("lib_tests"), std::fs::Permissions::from_mode(0o755)).unwrap();
    script(&tools.join("cargo"), &lane, &format!(r#"mkdir -p target/debug/deps && cp '{}' target/debug/deps/lib_tests && printf '{{"reason":"compiler-artifact","profile":{{"test":true}},"executable":"%s/target/debug/deps/lib_tests"}}\n' "$(pwd -P)""#, tools.join("lib_tests").display()));
    let path = format!("{}:{}", tools.display(), std::env::var("PATH").unwrap_or_default());
    std::fs::write(lane.join(".gitignore"), "target/\n").unwrap();
    std::fs::write(lane.join(".safe-push"), "[mutant]\n\trun = \"harness/scripts/mutant-tests.sh\"\n").unwrap();
    std::fs::create_dir_all(lane.join("src")).unwrap();
    std::fs::write(lane.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
    git_in(&lane, &["add", "."]);
    git_in(&lane, &["commit", "-qm", "base"]);
    let base = git_in(&lane, &["rev-parse", "HEAD"]);
    std::fs::write(lane.join(".safe-push"), format!("[mutant]\n\trun = \"{} land mutant-runs\"\n", crate::support::bin_exe!("CARGO_BIN_EXE_north"))).unwrap();
    std::fs::write(lane.join("src/lib.rs"), "pub fn f() -> u8 { 3 }\n").unwrap();
    std::fs::write(lane.join("src/lib_tests.rs"), "use super::*;\n#[test]\nfn guards_first() {\n    assert_eq!(f(), 3);\n}\n").unwrap();
    git_in(&lane, &["add", "."]);
    git_in(&lane, &["commit", "-qm", "first"]);
    let first = git_in(&lane, &["rev-parse", "HEAD"]);
    std::fs::write(lane.join("src/lib.rs"), "pub fn f() -> u8 { 5 }\n").unwrap();
    std::fs::write(lane.join("src/lib_tests.rs"), "use super::*;\n#[test]\nfn guards_first() {\n    assert_eq!(f(), 3);\n}\n#[test]\nfn idle_second() {\n    assert!(f() > 0);\n}\n").unwrap();
    git_in(&lane, &["add", "."]);
    git_in(&lane, &["commit", "-qm", "second"]);
    let second = git_in(&lane, &["rev-parse", "HEAD"]);

    let trees = lane.join("target/mutant");
    let [first_tree, second_tree, silent_tree] = ["first", "second", "silent"].map(|name| trees.join(name));
    git_in(&lane, &["worktree", "add", "-q", "--detach", first_tree.to_str().unwrap(), &first]);
    git_in(&lane, &["worktree", "add", "-q", "--detach", second_tree.to_str().unwrap(), &second]);
    git_in(&lane, &["worktree", "add", "-q", "--detach", silent_tree.to_str().unwrap(), &second]);
    std::fs::write(silent_tree.join("src/silent"), "").unwrap();
    let listed = [&first_tree, &second_tree, &silent_tree].map(|tree| tree.display().to_string()).join("\n");

    let ran = north_with(&home, &lane, &path, &["land", "mutant-runs"], &[("MUTANT_TESTS", "*"), ("MUTANT_TREES", listed.as_str())]);
    let printed = format!("{}{}", String::from_utf8_lossy(&ran.stdout), String::from_utf8_lossy(&ran.stderr));
    assert!(ran.status.success(), "land mutant-runs exits 0 once each tree's tests are run: {printed}");
    let recorded = |tree: &Path| std::fs::read_to_string(tree.join(".mutant-results")).unwrap_or_default();
    assert_eq!(recorded(&first_tree), "pass\tguards_first\n", "the wildcard records each test the first commit's executables list, and idle_second, which that commit does not have, is not judged there: {printed}");
    assert_eq!(recorded(&second_tree), "fail\tguards_first\npass\tidle_second\n", "a test whose own line says FAILED is recorded as a fail, and the second commit's idle test is judged on the second commit alone: {printed}");
    assert_eq!(recorded(&silent_tree), "", "a batch that exits non-zero without the test's own line records nothing, so it kills no mutant: {printed}");
    assert!(lane.join("target/mutant-src/target/debug/deps/lib_tests").is_file(), "the trees are mirrored into one source directory and built into its one target, not into each tree's own: {printed}");

    let scan = north_with(&home, &lane, &path, &["land", "scan", "test-admission", "run", &format!("{base}..{second}")], &[]);
    let scanned = format!("{}{}", String::from_utf8_lossy(&scan.stdout), String::from_utf8_lossy(&scan.stderr));
    assert_eq!(scan.status.code(), Some(1), "{scanned}");
    assert!(scanned.contains(&format!("in lane {}", &second[..12])), "on 2026-11-14 the fail-to-pass scan judged the lanes together and blamed them all for one lane's idle test: {scanned}");
    assert!(!scanned.contains(&format!("lane {}", &first[..12])), "{scanned}");
    assert!(!scanned.contains("could not run"), "on 2026-11-13 the fail-to-pass scan run from a worker's lane could not build its base, so it exited 75 with no verdict: {scanned}");
}
