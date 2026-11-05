use crate::foundation::shim::write_executable;
use super::tick::{review_name, tick_repo, TickEnv, Verdict};
use std::fs;
use std::process::Command;
use time::OffsetDateTime;

fn git(args: &[&str]) {
    assert!(Command::new("git").args(args).status().unwrap().success());
}

fn scratch(signal: &str) -> (tempfile::TempDir, TickEnv) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let checkout = root.join("code/makgora/main");
    fs::create_dir_all(&checkout).unwrap();
    let r = root.display();
    git(&["init", "-q", &checkout.display().to_string()]);
    git(&["init", "-q", "--bare", &format!("{r}/origin.git")]);
    git(&["-C", &checkout.display().to_string(), "remote", "add", "origin", &format!("{r}/origin.git")]);
    write_executable(&root.join("code-health"), format!("#!/usr/bin/env bash\necho \"$*\" >> {r}/calls\ncase $1 in\n  signal) {signal} ;;\n  report) echo \"hotspots: sim/physics.ts 412\" ;;\nesac\n"));
    fs::write(root.join("fleet.json"), "[]\n").unwrap();
    write_executable(&root.join("north"), format!("#!/usr/bin/env bash\necho \"$*\" >> {r}/north-calls\ncase \"$1 $2\" in\n  \"fleet list\") cat {r}/fleet.json ;;\n  \"fleet launch\") echo ok ;;\n  *) exit 2 ;;\nesac\n"));
    let env = TickEnv {
        code_root: root.join("code"),
        state_root: root.join("state"),
        handoffs: root.join("handoffs"),
        code_health: vec![root.join("code-health").display().to_string()],
        north: root.join("north").display().to_string(),
        dry_run: true,
        now: OffsetDateTime::now_utc(),
    };
    (dir, env)
}

fn tick(env: &TickEnv, lines: &mut Vec<String>) -> Verdict {
    tick_repo("makgora", env, &mut |line| lines.push(line)).verdict
}

fn lock(env: &TickEnv) -> std::path::PathBuf {
    env.state_root.join("makgora/review.lock")
}

#[test]
fn fire_starts_exactly_one_review_launch_and_a_held_lock_starts_none() {
    let (dir, env) = scratch("echo \"FIRE commits 104 >= 100\"");
    let mut lines = Vec::new();
    assert_eq!(tick(&env, &mut lines), Verdict::Started);
    assert_eq!(lines.len(), 1);
    let name = review_name("makgora", env.now);
    assert!(lines[0].starts_with("dry-run: "));
    assert!(lines[0].contains(&format!("fleet launch --provider claude --name {name} ")));
    assert!(lines[0].contains("--system-prompt "));
    assert!(!lines[0].contains("--account"));
    let brief = fs::read_to_string(env.handoffs.join(format!("{name}-brief.md"))).unwrap();
    assert!(brief.starts_with("# Code health review: makgora"));
    assert!(brief.split("\n\n").next().unwrap().contains("Surface: ts\nNeeds: local-state"));
    assert!(brief.contains("- FIRE commits 104 >= 100"));
    assert!(!brief.contains("{{"));
    assert!(lock(&env).exists());
    assert_eq!(tick(&env, &mut lines), Verdict::Held);
    assert_eq!(lines.len(), 1);
    let calls = fs::read_to_string(dir.path().join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|line| line.starts_with("signal ")).count(), 1);
}

#[test]
fn quiet_releases_the_lock_and_starts_nothing() {
    let (_dir, env) = scratch("echo quiet; exit 1");
    let mut lines = Vec::new();
    assert_eq!(tick(&env, &mut lines), Verdict::Quiet);
    assert!(lines.is_empty());
    assert!(!lock(&env).exists());
}

#[test]
fn a_stale_lock_with_no_live_review_node_is_retaken() {
    let (dir, env) = scratch("echo \"FIRE days 7\"");
    fs::create_dir_all(env.state_root.join("makgora")).unwrap();
    fs::write(lock(&env), "{}\n").unwrap();
    let later = TickEnv { now: env.now + time::Duration::hours(2), ..env };
    assert_eq!(tick(&later, &mut Vec::new()), Verdict::Started);
    fs::write(dir.path().join("fleet.json"), serde_json::json!([{ "name": review_name("makgora", later.now) }]).to_string()).unwrap();
    let after = TickEnv { now: later.now + time::Duration::minutes(10), ..later };
    assert_eq!(tick(&after, &mut Vec::new()), Verdict::Held);
}

#[test]
fn a_launch_refused_on_every_account_is_an_error_and_one_refused_for_a_saturated_host_is_deferred_and_both_release_the_lock() {
    let (dir, env) = scratch("echo \"FIRE days 7\"");
    let env = TickEnv { dry_run: false, ..env };
    write_executable(&dir.path().join("north"), "#!/usr/bin/env bash\nexit 1\n");
    assert_eq!(tick(&env, &mut Vec::new()), Verdict::Error);
    assert!(!lock(&env).exists());
    write_executable(&dir.path().join("north"), "#!/usr/bin/env bash\necho 'error: whiterabbit skipped: 5-minute load 36.1 over 24 cores; launch from another host, or keep it here with --local \"<reason>\"' >&2\nexit 1\n");
    assert_eq!(tick(&env, &mut Vec::new()), Verdict::Deferred);
    assert!(!lock(&env).exists());
}

#[test]
fn a_failed_fetch_is_an_error_that_releases_the_lock_not_quiet() {
    let (dir, env) = scratch("echo quiet; exit 1");
    let checkout = dir.path().join("code/makgora/main").display().to_string();
    git(&["-C", &checkout, "remote", "set-url", "origin", &dir.path().join("missing.git").display().to_string()]);
    assert_eq!(tick(&env, &mut Vec::new()), Verdict::Error);
    assert!(!lock(&env).exists());
}
