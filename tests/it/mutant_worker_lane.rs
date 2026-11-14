use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::support::bin_exe;

const BROKER: &str = r#"case "$(pwd -P)/" in "LANE"/*) ;; *) echo "north worker-sandbox broker: refused: $(pwd -P) is in no lane (PROJECT/worktrees/NAME) and no main checkout (PROJECT/main) of this host's projects, and a request is served only from one; run it from your lane" >&2; exit 75 ;; esac"#;

const TEST_BINARY: &str = r#"case "$1" in --list) echo "guards_the_change: test" ;; *) shift; if grep -q 'f() -> u8 { 3 }' src/lib.rs; then echo "test guards_the_change ... ok"; else echo "test guards_the_change ... FAILED"; exit 101; fi ;; esac"#;

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

fn north(home: &Path, directory: &Path, path: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(bin_exe!("CARGO_BIN_EXE_north"))
        .current_dir(directory)
        .args(arguments)
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
fn scan_from_a_worker_lane_reaches_a_verdict_from_both_admission_scans() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().canonicalize().unwrap();
    let lane = home.join("code/north/worktrees/lane");
    let tools = home.join("tools");
    std::fs::create_dir_all(&lane).unwrap();
    git_in(&lane, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("lib_tests"), format!("#!/bin/sh\n{TEST_BINARY}\n")).unwrap();
    std::fs::set_permissions(tools.join("lib_tests"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(bin_exe!("CARGO_BIN_EXE_north"), tools.join("north")).unwrap();
    script(&tools.join("cargo"), &lane, &format!(r#"mkdir -p target/debug/deps && cp '{}' target/debug/deps/lib_tests && printf '{{"reason":"compiler-artifact","profile":{{"test":true}},"executable":"%s/target/debug/deps/lib_tests"}}\n' "$(pwd -P)""#, tools.join("lib_tests").display()));
    script(&lane.join("bin/land-shell"), &lane, r#"shift 2; case " $* " in *" --list "*) echo "src/lib.rs:1:21: replace f -> u8 with 0"; exit 0 ;; esac
while [ $# -gt 0 ]; do [ "$1" = --output ] && out=$2; shift; done
mkdir -p "$out/mutants.out" && echo "src/lib.rs:1:21: replace f -> u8 with 0" >"$out/mutants.out/caught.txt""#);
    let path = format!("{}:{}", tools.display(), std::env::var("PATH").unwrap_or_default());
    std::fs::write(lane.join(".safe-push"), "[mutant]\n\trun = north land mutant-runs\n\tsource = \":(glob)src/**/*.rs\"\n\tsource = \":(exclude,glob)**/*tests*.rs\"\n").unwrap();
    std::fs::write(lane.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(lane.join("src")).unwrap();
    std::fs::write(lane.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
    git_in(&lane, &["add", "."]);
    git_in(&lane, &["commit", "-qm", "base"]);
    let base = git_in(&lane, &["rev-parse", "HEAD"]);
    std::fs::write(lane.join("src/lib.rs"), "pub fn f() -> u8 { 3 }\n").unwrap();
    std::fs::write(lane.join("src/lib_tests.rs"), "use super::*;\n#[test]\nfn guards_the_change() {\n    assert_eq!(f(), 3);\n}\n").unwrap();
    git_in(&lane, &["add", "."]);
    git_in(&lane, &["commit", "-qm", "lane"]);
    let range = format!("{base}..{}", git_in(&lane, &["rev-parse", "HEAD"]));

    let fail_to_pass = north(&home, &lane, &path, &["land", "scan", "test-admission", "run", &range]);
    let printed = format!("{}{}", String::from_utf8_lossy(&fail_to_pass.stdout), String::from_utf8_lossy(&fail_to_pass.stderr));
    assert!(fail_to_pass.status.code().is_some_and(|code| code <= 1) && !printed.contains("could not run"), "on 2026-11-13 the fail-to-pass scan run from a worker's lane built in a scratch copy under ~/.cache/north/mutant-cargo, which the worker sandbox's broker serves no build from, so it exited 75 with no verdict: {printed}");
    assert!(!lane.join("target/mutant-cargo").exists(), "on 2026-11-13 the fail-to-pass scan left its base tree's build under target/mutant-cargo in the lane, 5.5 GB per checkout and offload slot");

    let swept = north(&home, &lane, &path, &["land", "scan", "test-admission", "kills", "--here", &range]);
    let printed = format!("{}{}", String::from_utf8_lossy(&swept.stdout), String::from_utf8_lossy(&swept.stderr));
    assert!(swept.status.code().is_some_and(|code| code <= 1) && printed.contains("mutant kill: ✓"), "on 2026-11-13 the mutant kill scan run from a worker's lane swept a worktree under ~/.cache, which the worker sandbox's broker serves no build from and the lane's build cannot be hard-linked into across devices, so it reached no verdict: {printed}");
    assert!(!lane.join("target/mutant-cargo").exists(), "on 2026-11-13 the mutant kill scan left a build under target/mutant-cargo in the lane, 5.5 GB per checkout and offload slot");
}
