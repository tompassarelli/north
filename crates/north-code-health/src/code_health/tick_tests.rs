use crate::foundation::shim::write_executable;
use super::tick::{tick_repo, TickEnv, Verdict, ENROLLED};
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
        graph: root.join("graph.db"),
        handoffs: root.join("handoffs"),
        code_health: vec![root.join("code-health").display().to_string()],
        north: root.join("north").display().to_string(),
        dry_run: true,
        now: OffsetDateTime::now_utc(),
        paused: Vec::new(),
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
fn every_repo_of_a_project_with_paused_true_gets_no_review_launch_while_other_enrolled_repos_still_tick() {
    let (dir, env) = scratch("echo \"FIRE days 7\"");
    let orchestration: toml_edit::DocumentMut = "[projects.north]\nrepos = [\"north\"]\n[projects.galileo]\npaused = true\nrepos = [\"galileo\"]\n[projects.makgora]\npaused = true\nrepos = [\"makgora\", \"makgora-client\", \"south\"]\ndispatch_repos = [\"makgora\"]\n[projects.wisp]\npaused = true\nrepos = [\"wisp\"]\n".parse().unwrap();
    let env = TickEnv { dry_run: false, paused: north_base::platform::orchestration::set_aside_repos(&orchestration), ..env };
    let verdicts: Vec<(&str, Verdict)> = ENROLLED.iter().map(|&repo| (repo, tick_repo(repo, &env, &mut |_| {}).verdict)).collect();
    assert_eq!(verdicts, [("north", Verdict::Missing), ("makgora", Verdict::Paused), ("wisp", Verdict::Paused), ("padbridge", Verdict::Missing), ("makgora-client", Verdict::Paused)]);
    assert!(!lock(&env).exists());
    assert!(fs::read_to_string(dir.path().join("north-calls")).unwrap_or_default().is_empty());
}

#[test]
fn a_review_lock_named_by_an_exited_session_past_the_launch_grace_is_taken_over_and_a_live_one_still_holds() {
    let (dir, env) = scratch("echo quiet");
    let env = TickEnv { now: OffsetDateTime::now_utc() + time::Duration::hours(2), ..env };
    fs::create_dir_all(lock(&env).parent().unwrap()).unwrap();
    fs::write(lock(&env), "{\"name\":\"code-health-review-makgora-20261104-2100\"}\n").unwrap();
    fs::write(dir.path().join("fleet.json"), "[{\"name\":\"code-health-review-makgora-20261104-2100\",\"status\":\"running\"}]\n").unwrap();
    assert_eq!(tick(&env, &mut Vec::new()), Verdict::Held);
    fs::write(dir.path().join("fleet.json"), "[{\"name\":\"code-health-review-makgora-20261104-2100\",\"status\":\"exited\"}]\n").unwrap();
    assert_ne!(tick(&env, &mut Vec::new()), Verdict::Held);
}

// KEEP(bug): launches refused for room (the spawn guard's CPU refusal, a held placement) failed code-health-review.service instead of deferring; north#721
#[test]
fn a_launch_refused_for_room_with_exit_75_defers_to_the_next_tick_and_frees_the_lock() {
    let (dir, env) = scratch("echo \"FIRE days 7\"");
    let r = dir.path().display();
    write_executable(&dir.path().join("north"), format!("#!/usr/bin/env bash\necho \"$*\" >> {r}/north-calls\ncase \"$1 $2\" in\n  \"fleet list\") cat {r}/fleet.json ;;\n  \"fleet launch\") echo 'error: held: whiterabbit skipped: load 41 on 24 threads' >&2; exit 75 ;;\n  *) exit 2 ;;\nesac\n"));
    let env = TickEnv { dry_run: false, ..env };
    assert_eq!(tick(&env, &mut Vec::new()), Verdict::Deferred);
    assert!(!lock(&env).exists());
}
