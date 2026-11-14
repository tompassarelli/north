use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::{fail_to_pass, kills};

const BROKER: &str = r#"case "$(pwd -P)/" in "LANE"/*) ;; *) echo "north worker-sandbox broker: refused: $(pwd -P) is in no lane (PROJECT/worktrees/NAME) and no main checkout (PROJECT/main) of this host's projects, and a request is served only from one; run it from your lane" >&2; exit 75 ;; esac"#;

fn git_in(directory: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
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

#[test]
fn the_crossing_count_names_only_the_lane_that_changes_it() {
    let root = tempfile::tempdir().unwrap();
    let lane = root.path().canonicalize().unwrap().join("code/north/worktrees/lane");
    let tools = root.path().join("tools");
    std::fs::create_dir_all(&lane).unwrap();
    git_in(&lane, &["init", "-q", "-b", "main"]);
    script(&tools.join("cargo"), &lane, r#"for word; do last=$word; done; case $last in idle_second) echo "test lib_tests::$last ... ok" ;; *) echo "test lib_tests::$last ... FAILED" ;; esac"#);
    script(&lane.join("bin/land-shell"), &lane, r#"shift 2; case " $* " in *" --list "*) echo "src/lib.rs:1:21: replace f -> u8 with 0"; exit 0 ;; esac
while [ $# -gt 0 ]; do [ "$1" = --output ] && out=$2; shift; done
mkdir -p "$out/mutants.out" && echo "src/lib.rs:1:21: replace f -> u8 with 0" >"$out/mutants.out/caught.txt""#);
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/scripts/mutant-tests.sh").canonicalize().unwrap();
    let run = format!("PATH='{}':\"$PATH\" MUTANT_SLOTS=1 bash '{}'", tools.display(), runner.display());
    std::fs::write(lane.join(".safe-push"), format!("[mutant]\n\trun = \"{}\"\n\tsource = \":(glob)src/**/*.rs\"\n\tsource = \":(exclude,glob)**/*tests*.rs\"\n", run.replace('\\', "\\\\").replace('"', "\\\""))).unwrap();
    std::fs::write(lane.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(lane.join("src")).unwrap();
    std::fs::write(lane.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
    git_in(&lane, &["add", "."]);
    git_in(&lane, &["commit", "-qm", "base"]);
    let base = git_in(&lane, &["rev-parse", "HEAD"]);
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

    let mut ran = Vec::new();
    let code = fail_to_pass(&lane, &format!("{base}..{second}"), &mut ran);
    let printed = String::from_utf8_lossy(&ran);
    assert_eq!(code, 1, "{printed}");
    assert!(printed.contains(&format!("in lane {}", &second[..12])), "on 2026-11-14 the fail-to-pass scan run after a batch judged the lanes together and blamed them all for one lane's idle test: {printed}");
    assert!(!printed.contains(&format!("lane {}", &first[..12])), "{printed}");
    assert!(!printed.contains("could not run"), "on 2026-11-13 the fail-to-pass scan run from a worker's lane built in a scratch copy under ~/.cache/north/mutant-cargo, which the worker sandbox's broker serves no build from, so it exited 75 with no verdict: {printed}");
    assert!(!lane.join("target/mutant-cargo").exists(), "on 2026-11-13 the fail-to-pass scan left its base tree's build under target/mutant-cargo in the lane, 5.5 GB per checkout and offload slot");

    let mut swept = Vec::new();
    let code = kills(&lane, &["--here".to_owned(), format!("{base}..{second}")], &mut swept);
    let printed = String::from_utf8_lossy(&swept);
    assert!(code <= 1 && printed.contains("mutant kill: ✓"), "on 2026-11-13 the mutant kill scan run from a worker's lane swept a worktree under ~/.cache, which the worker sandbox's broker serves no build from and the lane's build cannot be hard-linked into across devices, so it reached no verdict: {printed}");
    assert!(!lane.join("target/mutant-cargo").exists(), "on 2026-11-13 the mutant kill scan left a build under target/mutant-cargo in the lane, 5.5 GB per checkout and offload slot");
}
