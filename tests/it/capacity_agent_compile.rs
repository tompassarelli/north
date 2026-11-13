use crate::capacity_host::{Exclusive, Host};
use std::fs;
use std::process::Output;

const SYSTEMD_RUN: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";
const SYSTEMCTL: &str = "#!/bin/sh\ncase \" $* \" in *\" show \"*) echo inactive ;; esac\n";
const DARK_SSH: &str = "#!/bin/sh\nexit 255\n";
const ORCHESTRATION: &str = "[hosts.laptop]\nreaches = [\"farhost\"]\n\n[hosts.farhost]\nssh = \"farhost\"\noffload_builds = true\n";
const FULL: &str = "some avg10=95.00 avg60=95.00 avg300=95.00 total=1\n";
const IDLE: &str = "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";

fn host() -> Host {
    let host = Host::checkout("agent-compile-", &[("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL), ("ssh", DARK_SSH)]);
    fs::write(host.path("orchestration.toml"), ORCHESTRATION).unwrap();
    fs::create_dir_all(host.path("real")).unwrap();
    fs::write(host.path("self-cgroup"), "0::/user.slice/user-1000.slice/session-1.scope\n").unwrap();
    let marker = host.path("compiled-here");
    let real = format!("#!/bin/sh\necho \"$@\" >>{}\n", marker.display());
    north_core::foundation::shim::write_executable(&host.path("real/cargo"), &real);
    host
}

fn gate(host: &Host, session: Option<&str>, arguments: &[&str]) -> Output {
    let mut command = host.command(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
    command.env("RESIDENT_CAP_HOST", "laptop");
    command.args(["capacity", "gate", "cargo"]).args(arguments).env("PATH", format!("{}:{}:{}", host.path("real").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default()));
    command.env("AGENT_CAPACITY_SELF_CGROUP", host.path("self-cgroup"));
    for key in ["NORTH_FLEET_SESSION", "NORTH_FLEET_SCOPE", "AGENT_CAPACITY_NEED"] {
        command.env_remove(key);
    }
    if let Some(session) = session {
        command.env("NORTH_FLEET_SESSION", session);
    }
    command.output_exclusive().unwrap()
}

fn compiled(host: &Host) -> String {
    fs::read_to_string(host.path("compiled-here")).unwrap_or_default()
}

#[test]
fn a_fleet_sessions_compile_from_any_entry_is_refused_only_when_no_build_host_and_not_this_host_has_room() {
    let host = host();
    fs::write(host.path("pressure"), FULL).unwrap();
    let refused_on_a_full_host = |refusal: fn(&Host)| {
        let host = self::host();
        fs::write(host.path("pressure"), FULL).unwrap();
        refusal(&host);
        assert_eq!(compiled(&host), "", "a fleet session compiled on a laptop with no room");
    };
    std::thread::scope(|scope| {
        let depbox = scope.spawn(|| refused_on_a_full_host(refused_inside_depbox));
        let hook = scope.spawn(|| refused_on_a_full_host(refused_through_the_hook));
        for subcommand in [&["build"][..], &["check", "-p", "x"], &["clippy"], &["nextest", "run"], &["test"]] {
            let refused = gate(&host, Some("n634-x"), subcommand);
            let stderr = String::from_utf8_lossy(&refused.stderr);
            assert_eq!(refused.status.code(), Some(75), "{subcommand:?}: {stderr}");
            assert!(stderr.contains("no build host took the job") && stderr.contains("farhost: unreachable"), "{subcommand:?}: {stderr}");
        }
        for joined in [depbox.join(), hook.join()] {
            joined.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        }
    });
    assert_eq!(compiled(&host), "", "a fleet session compiled on a laptop with no room");
    fs::write(host.path("pressure"), IDLE).unwrap();
    let waited = gate(&host, Some("n634-x"), &["test"]);
    assert_eq!(waited.status.code(), Some(0), "a fleet compile with room here: {}", String::from_utf8_lossy(&waited.stderr));
    assert_eq!(compiled(&host), "test\n", "with room here the fleet compile runs here");
    let mine = gate(&host, None, &["build"]);
    assert_eq!(mine.status.code(), Some(0), "{}", String::from_utf8_lossy(&mine.stderr));
    assert_eq!(compiled(&host), "test\nbuild\n");
    let admitted: i64 = rusqlite::Connection::open(host.path("capacity.db")).unwrap().query_row("SELECT count(*) FROM capacity_admissions WHERE decision = 'RESERVED'", [], |row| row.get(0)).unwrap();
    assert_eq!(admitted, 1, "the fleet build admitted here is one admission row, kept for the ledger; a shell outside any session leases nothing (north#798)");
}

#[test]
fn a_fleet_sessions_cargo_that_compiles_nothing_or_needs_this_host_runs_here() {
    let host = host();
    let mut needy = host.command(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
    needy.env("RESIDENT_CAP_HOST", "laptop");
    needy.env("AGENT_CAPACITY_SELF_CGROUP", host.path("self-cgroup"));
    needy.args(["capacity", "gate", "cargo", "build"]).env("PATH", format!("{}:{}:{}", host.path("real").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default())).env("NORTH_FLEET_SESSION", "n634-x").env("AGENT_CAPACITY_NEED", "gpu");
    assert_eq!(needy.output_exclusive().unwrap().status.code(), Some(0));
    assert_eq!(gate(&host, Some("n634-x"), &["metadata"]).status.code(), Some(0));
    assert_eq!(gate(&host, Some("n634-x"), &["fmt"]).status.code(), Some(0));
    assert_eq!(compiled(&host), "build\nmetadata\nfmt\n");
}

#[test]
fn a_fleet_sessions_cargo_with_no_real_cargo_on_path_is_refused_not_run_in_the_dev_shell() {
    let host = host();
    fs::write(host.path("pressure"), FULL).unwrap();
    let mut command = host.command(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
    command.env("RESIDENT_CAP_HOST", "laptop");
    command.args(["capacity", "gate", "cargo", "test"]).env("PATH", host.path("bin")).env("AGENT_CAPACITY_SELF_CGROUP", host.path("self-cgroup")).env("NORTH_FLEET_SESSION", "n634-x");
    let refused = command.output_exclusive().unwrap();
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(75), "{stderr}");
    assert!(stderr.contains("no build host took the job"), "{stderr}");
}

fn fleet(host: &Host, program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = host.command(program);
    command.env("PATH", format!("{}:{}:{}", host.path("real").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default()));
    command.env("AGENT_CAPACITY_SELF_CGROUP", host.path("self-cgroup")).env("HOME", host.path("home")).env("NORTH_FLEET_SESSION", "n671-x").env("RESIDENT_CAP_HOST", "laptop");
    for key in ["NORTH_FLEET_SCOPE", "AGENT_CAPACITY_NEED", "DEPBOX", "DEPBOX_ADMITTED", "CARGO_TARGET_DIR", "NORTH_BUILD_OFFLOADED"] {
        command.env_remove(key);
    }
    command
}

fn refused_for_want_of_a_build_host(output: &Output, what: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(75), "{what}: {stderr}");
    assert!(stderr.contains("no build host took the job") && stderr.contains("farhost: unreachable"), "{what}: {stderr}");
}

fn refused_inside_depbox(host: &Host) {
    let project = host.path("home/code/proj/main");
    fs::create_dir_all(&project).unwrap();
    assert!(std::process::Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap().success());
    std::os::unix::fs::symlink(crate::support::bin_exe!("CARGO_BIN_EXE_north"), host.path("real/north")).unwrap();
    for command in [&["run", "--", "cargo", "build"][..], &["run", "--", "nix", "develop", "-c", "cargo", "test", "--locked", "--offline"]] {
        let mut depbox = fleet(host, crate::support::manifest_dir().join("bin/depbox"));
        let refused = depbox.current_dir(&project).args(command).output_exclusive().unwrap();
        refused_for_want_of_a_build_host(&refused, &format!("depbox {command:?}"));
    }
    let carried = fleet(host, crate::support::bin_exe!("CARGO_BIN_EXE_north")).args(["capacity", "carry", "--list", "--into", "sandbox"]).output_exclusive().unwrap();
    assert!(String::from_utf8_lossy(&carried.stdout).lines().any(|name| name == "NORTH_FLEET_SESSION"), "depbox drops the fleet session");
    let inside = fleet(host, crate::support::bin_exe!("CARGO_BIN_EXE_north")).args(["capacity", "gate", "cargo", "build"]).env("DEPBOX", format!("run:{}", project.display())).output_exclusive().unwrap();
    let stderr = String::from_utf8_lossy(&inside.stderr);
    assert_eq!(inside.status.code(), Some(75), "{stderr}");
    assert!(stderr.contains("inside depbox reaches no build host"), "{stderr}");
}

fn refused_through_the_hook(host: &Host) {
    for program in ["rustc", "cargo-nextest"] {
        north_core::foundation::shim::write_executable(&host.path(&format!("real/{program}")), format!("#!/bin/sh\necho {program} \"$@\" >>{}\n", host.path("compiled-here").display()));
    }
    fs::write(host.path("activation.active"), "hook heavy-command-guard\n").unwrap();
    for typed in ["rustc src/main.rs -o main", "cargo-nextest nextest run"] {
        let request = serde_json::json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": typed }, "cwd": host.path("").display().to_string() });
        let mut hook = fleet(host, crate::support::bin_exe!("CARGO_BIN_EXE_north"));
        hook.env("NORTH_AGENT_ACTIVE", host.path("activation.active"));
        hook.env("AGENT_HOOK_ERRORS", host.path("hook-errors.tsv"));
        fs::write(host.path("hook-request.json"), request.to_string()).unwrap();
        let output = hook.args(["hook", "pre-bash"]).stdin(fs::File::open(host.path("hook-request.json")).unwrap()).output_exclusive().unwrap();
        let routed = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok().and_then(|answer| answer["hookSpecificOutput"]["updatedInput"]["command"].as_str().map(str::to_owned)).unwrap_or_else(|| panic!("{typed}: the hook did not route it: {}\nstdout: {}\nstderr: {}\nhook errors: {}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr), fs::read_to_string(host.path("hook-errors.tsv")).unwrap_or_default()));
        assert!(routed.starts_with("north capacity host run "), "{typed}: {routed}");
        let ran = fleet(host, "bash").args(["-c", &routed.replacen("north", crate::support::bin_exe!("CARGO_BIN_EXE_north"), 1)]).output_exclusive().unwrap();
        refused_for_want_of_a_build_host(&ran, typed);
    }
}
