use std::fs;
use std::path::Path;
use std::process::Command;

use super::{Verdict, added_invocations, judge};

const SERVE: &str = r#"
            ("--once", _) => once = true,
            ("--repo", Some(value)) => {}
            ("--checker", Some(value)) => {}
            ("--depth", Some(value)) if matches!(value.as_str(), "1" | "2") => {}
"#;
const SERVE_WITH_ALL: &str = r#"
            ("--once", _) => once = true,
            ("--all", _) => every = true,
            ("--repo", Some(value)) => {}
            ("--checker", Some(value)) => {}
"#;
const MODULE_BEFORE: &str = "  ExecStart = \"${northBin}/north land serve --repo ${homeDir}/code/north/main\";\n";
const MODULE_AFTER: &str = "  ExecStart = \"${northBin}/north land serve --all --repo ${homeDir}/code/north/main\";\n";
const SERVE_PATH: &str = "crates/north-landing/src/land/serve.rs";
const MODULE_PATH: &str = "modules/north-land/default.nix";

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

fn init() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Floor"]);
    git(dir.path(), &["config", "user.email", "floor@example.invalid"]);
    dir
}

fn north() -> (tempfile::TempDir, String) {
    let dir = init();
    let old = commit(
        dir.path(),
        &[
            ("src/cli/tools.rs", "(\"land\", crate::land::run),\n(\"fleet\", crate::fleet::run),\n(\"config\", crate::agent_catalog::run),\n(\"watch\", crate::watch::run),\n"),
            (SERVE_PATH, SERVE),
            ("crates/north-landing/src/watch/mod.rs", "match arguments { [\"--once\"] => false, _ => true }\n"),
            ("crates/north-landing/src/land/mod.rs", "Some((command, rest)) if command == \"serve\" => serve::run(rest),\n"),
            ("crates/north-core/src/agent_catalog.rs", "#[path = \"catalog/registration_checks.rs\"]\nmod registration_checks;\n\"agents\" => 0,\n\"check-hook-paths\" => registration_checks::parse(rest),\n"),
            ("crates/north-core/src/catalog/registration_checks.rs", "\"--claude\" => 1, \"--codex\" => 2,\n"),
            ("crates/north-landing/src/land/queue_tests.rs", "(\"--all\", _) => {}\n"),
            ("crates/north-core/src/lanes/reap.rs", "\"--all\" => all = true,\n"),
        ],
    );
    (dir, old)
}

fn nixos() -> (tempfile::TempDir, String, String) {
    let dir = init();
    let base = commit(dir.path(), &[(MODULE_PATH, MODULE_BEFORE), ("flake.nix", "{ description = \"the machine\"; }\n")]);
    let head = commit(dir.path(), &[(MODULE_PATH, MODULE_AFTER)]);
    (dir, base, head)
}

fn state(current: &str, hosts: &[(&str, &str)]) -> tempfile::TempDir {
    let state = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{current}"), state.path().join("current")).unwrap();
    let rows: Vec<String> = hosts.iter().map(|(host, runtime)| format!(r#"{{"host": "{host}", "runtime": "{runtime}"}}"#)).collect();
    fs::write(state.path().join("runtime-settle.json"), format!(r#"{{"hosts": [{}]}}"#, rows.join(", "))).unwrap();
    state
}

#[test]
fn the_2026_11_05_0206_north_land_serve_all_change_is_refused_until_every_live_runtime_accepts_it() {
    let (north_dir, old) = north();
    let (nixos_dir, base, head) = nixos();
    let added = added_invocations(nixos_dir.path(), &base, &head).unwrap();
    assert_eq!(added.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), [MODULE_PATH]);

    let lagging = state(&old, &[("gex45", &old)]);
    let Verdict::Refused(lines) = judge(nixos_dir.path(), Some(north_dir.path()), lagging.path(), &base, &head).unwrap() else { panic!("the invocation went live before north had --all") };
    let short = &old[..8];
    assert_eq!(lines[0], format!("runtime-floor-gate: {MODULE_PATH}: north land serve --all is missing from the oldest live runtime {short} (gex45)"));

    let with_all = commit(north_dir.path(), &[(SERVE_PATH, SERVE_WITH_ALL)]);
    let half = state(&with_all, &[("whiterabbit", &with_all), ("gex45", &old)]);
    let Verdict::Refused(lines) = judge(nixos_dir.path(), Some(north_dir.path()), half.path(), &base, &head).unwrap() else { panic!("gex45 still runs a runtime that exits 64 on --all") };
    assert!(lines[0].ends_with(&format!("the oldest live runtime {short} (gex45)")), "{lines:?}");

    let settled = state(&with_all, &[("whiterabbit", &with_all), ("gex45", &with_all)]);
    assert_eq!(judge(nixos_dir.path(), Some(north_dir.path()), settled.path(), &base, &head).unwrap(), Verdict::Passed);
}

const JOB_RUN_PATH: &str = "modules/job-run/default.nix";
const LIVENESS_PATH: &str = "modules/delivery-liveness/default.nix";
const HEALTH_PATH: &str = "modules/code-health/default.nix";

fn floor_north() -> (tempfile::TempDir, String) {
    let dir = init();
    let old = commit(
        dir.path(),
        &[
            ("src/cli/tools.rs", "(\"land\", crate::land::run),\n(\"code-health\", crate::code_health::run),\n"),
            ("crates/north-core/src/code_health/mod.rs", "match command { \"report\" => report(), _ => usage() }\n"),
        ],
    );
    commit(
        dir.path(),
        &[
            ("src/cli/tools.rs", "(\"land\", crate::land::run),\n(\"code-health\", crate::code_health::run),\n(\"job-run\", crate::job_run::run),\n"),
            ("crates/north-core/src/code_health/mod.rs", "match command { \"report\" => report(), \"tick\" => tick(), _ => usage() }\n"),
        ],
    );
    (dir, old)
}

fn replay(files: &[(&str, &str, &str)]) -> (tempfile::TempDir, String, String) {
    let dir = init();
    let before: Vec<(&str, &str)> = files.iter().map(|(path, before, _)| (*path, *before)).collect();
    let after: Vec<(&str, &str)> = files.iter().map(|(path, _, after)| (*path, *after)).collect();
    let base = commit(dir.path(), &before);
    let head = commit(dir.path(), &after);
    (dir, base, head)
}

fn refused(repo: &Path, north: &Path, old: &str, base: &str, head: &str) -> Vec<String> {
    let live = state(old, &[("laptop", old)]);
    let Verdict::Refused(lines) = judge(repo, Some(north), live.path(), base, head).unwrap() else { panic!("the gate passed a verb the floor runtime lacks") };
    lines
}

#[test]
fn runtime_floor_refuses_the_e7619134_job_run_through_a_nix_northbin_path() {
    let (north_dir, old) = floor_north();
    let (dir, base, head) = replay(&[
        (
            JOB_RUN_PATH,
            "  ExecStart = \"${pkgs.bash}/bin/bash ${northBin}/job-run --path ${\n    recordPath\n  } --user ${username} ${name} -- ${cmd}\";\n",
            "  ExecStart = \"${pkgs.bash}/bin/bash ${northBin}/north job-run --path ${\n    recordPath\n  } --user ${username} ${name} -- ${cmd}\";\n",
        ),
        (
            LIVENESS_PATH,
            "  jobRun = name: cmd:\n    \"${pkgs.bash}/bin/bash ${northBin}/job-run --path ${recordPath} --user ${username} ${name} -- ${cmd}\";\n",
            "  jobRun = name: cmd:\n    \"${pkgs.bash}/bin/bash ${northBin}/north job-run --path ${recordPath} --user ${username} ${name} -- ${cmd}\";\n",
        ),
    ]);
    let lines = refused(dir.path(), north_dir.path(), &old, &base, &head);
    assert!(lines.iter().any(|line| line.starts_with(&format!("runtime-floor-gate: {JOB_RUN_PATH}: north job-run is missing"))), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with(&format!("runtime-floor-gate: {LIVENESS_PATH}: north job-run is missing"))), "{lines:?}");
    assert!(lines.iter().any(|line| line.contains("bash cannot run the north binary") && line.ends_with("north job-run")), "{lines:?}");
}

#[test]
fn runtime_floor_refuses_the_15158378_code_health_tick_the_floor_runtime_lacks() {
    let (north_dir, old) = floor_north();
    let (dir, base, head) = replay(&[(
        HEALTH_PATH,
        "  ExecStart = jobRun \"code-health-review\" \"${northBin}/code-health-tick\";\n",
        "  ExecStart = jobRun \"code-health-review\" \"${northBin}/north code-health tick\";\n",
    )]);
    let lines = refused(dir.path(), north_dir.path(), &old, &base, &head);
    assert!(lines.iter().any(|line| line.starts_with(&format!("runtime-floor-gate: {HEALTH_PATH}: north code-health tick is missing"))), "{lines:?}");
}
