use crate::foundation::shim::write_executable;
use super::tick::{tick_repo, TickEnv, Verdict};
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

