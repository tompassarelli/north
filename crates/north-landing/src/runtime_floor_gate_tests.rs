use std::fs;
use std::path::Path;
use std::process::Command;

use super::{Floor, calls, commands, live_runtimes, oldest, violations};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(repo).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "step"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn repo() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Floor"]);
    git(dir.path(), &["config", "user.email", "floor@example.invalid"]);
    let old = commit(dir.path(), &[("src/cli/mod.rs", "match command { \"language-gate\" => 1, _ => 0 }\n"), ("bin/safe-push", "#!/usr/bin/env bash\nnorth language-gate --repo .\n")]);
    (dir, old)
}

#[test]
fn calls_name_each_north_subcommand_outside_comments() {
    assert_eq!(calls("  north build-cache-gate --repo \"$root\" || x=$(north dep-gate a b)"), ["build-cache-gate", "dep-gate"]);
    assert_eq!(calls("exec north land enqueue \"$@\""), ["land"]);
    assert!(calls("  # north build-cache-gate is new").is_empty());
    assert!(calls("bin/northstar go; ~/north go").is_empty());
}

#[test]
fn a_script_calling_a_subcommand_landed_in_the_same_change_is_refused_against_its_base() {
    let (dir, old) = repo();
    let head = commit(
        dir.path(),
        &[
            ("src/cli/mod.rs", "match command { \"language-gate\" => 1, \"build-cache-gate\" => 2, _ => 0 }\n"),
            ("bin/safe-push", "#!/usr/bin/env bash\nnorth language-gate --repo .\nnorth build-cache-gate --repo .\n"),
        ],
    );
    assert!(commands(dir.path(), &head).contains("build-cache-gate"));
    let floor = oldest(dir.path(), Vec::new(), &old);
    assert_eq!(floor, Floor { sha: old.clone(), host: "the landing's base".to_owned() });
    assert_eq!(violations(dir.path(), &old, &head, &floor.sha).unwrap(), ["bin/safe-push: north build-cache-gate"]);
}

#[test]
fn a_call_is_refused_while_any_live_runtime_predates_its_subcommand() {
    let (dir, old) = repo();
    let added = commit(dir.path(), &[("src/cli/mod.rs", "match command { \"language-gate\" => 1, \"build-cache-gate\" => 2, _ => 0 }\n")]);
    let head = commit(dir.path(), &[("bin/safe-push", "#!/usr/bin/env bash\nnorth build-cache-gate --repo .\n")]);
    let state = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{added}"), state.path().join("current")).unwrap();
    let record = format!(r#"{{"hosts": [{{"host": "gex45", "runtime": "{old}"}}, {{"host": "nexus", "runtime": null}}, {{"host": "gone", "runtime": "{}"}}]}}"#, "f".repeat(40));
    fs::write(state.path().join("runtime-settle.json"), record).unwrap();
    let live = live_runtimes(state.path());
    assert_eq!(live.len(), 3);
    let floor = oldest(dir.path(), live, &added);
    assert_eq!(floor, Floor { sha: old, host: "gex45".to_owned() });
    assert_eq!(violations(dir.path(), &added, &head, &floor.sha).unwrap(), ["bin/safe-push: north build-cache-gate"]);
    let caught_up = oldest(dir.path(), vec![Floor { sha: added.clone(), host: "gex45".to_owned() }, Floor { sha: head.clone(), host: "this host".to_owned() }], &added);
    assert_eq!(caught_up.sha, added);
    assert!(violations(dir.path(), &added, &head, &caught_up.sha).unwrap().is_empty());
}

#[test]
fn a_probed_call_and_a_call_outside_scripts_pass() {
    let (dir, old) = repo();
    let head = commit(
        dir.path(),
        &[
            ("src/cli/mod.rs", "match command { \"language-gate\" => 1, \"build-cache-gate\" => 2, \"runtime-floor-gate\" => 3, _ => 0 }\n"),
            ("bin/safe-push", "#!/usr/bin/env bash\nnorth build-cache-gate --help >/dev/null 2>&1 || exit 1\nnorth build-cache-gate --repo .\nnorth_offers runtime-floor-gate && north runtime-floor-gate --repo .\n"),
            ("docs/features.md", "north build-cache-gate --repo DIR\n"),
        ],
    );
    assert!(violations(dir.path(), &old, &head, &old).unwrap().is_empty());
}
