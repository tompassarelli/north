use crate::capacity_host::{Exclusive, Host};
use std::fs;
use std::process::Output;

const SYSTEMD_RUN: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";
const SYSTEMCTL: &str = "#!/bin/sh\ncase \" $* \" in *\" show \"*) echo inactive ;; esac\n";
const DARK_SSH: &str = "#!/bin/sh\nexit 255\n";
const ORCHESTRATION: &str = "[hosts.laptop]\nreaches = [\"farhost\"]\n\n[hosts.farhost]\nssh = \"farhost\"\noffload_builds = true\n";

fn host() -> Host {
    let host = Host::new("agent-compile-", &[("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL), ("ssh", DARK_SSH)]);
    fs::write(host.path("orchestration.toml"), ORCHESTRATION).unwrap();
    fs::create_dir_all(host.path("real")).unwrap();
    fs::write(host.path("self-cgroup"), "0::/user.slice/user-1000.slice/session-1.scope\n").unwrap();
    let marker = host.path("compiled-here");
    let real = format!("#!/bin/sh\necho \"$@\" >>{}\n", marker.display());
    north_core::foundation::shim::write_executable(&host.path("real/cargo"), &real);
    host
}

fn gate(host: &Host, session: Option<&str>, arguments: &[&str]) -> Output {
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    command.env("RESIDENT_CAP_HOST", "laptop");
    command.args(["capacity", "gate", "cargo"]).args(arguments).env("PATH", format!("{}:{}", host.path("real").display(), host.path("bin").display()));
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

// KEEP(contract): a fleet session's cargo build on a host that does not lease builds is refused with the build hosts' reason when none has room, never compiled here, while a shell outside any session runs the same cargo locally; north#634
#[test]
fn a_fleet_sessions_cargo_build_is_refused_when_no_build_host_has_room_and_another_shell_builds_locally() {
    let host = host();
    for subcommand in [&["build"][..], &["check", "-p", "x"], &["clippy"], &["nextest", "run"], &["test"]] {
        let refused = gate(&host, Some("n634-x"), subcommand);
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(75), "{subcommand:?}: {stderr}");
        assert!(stderr.contains("no build host took the job") && stderr.contains("farhost: unreachable"), "{subcommand:?}: {stderr}");
    }
    assert_eq!(compiled(&host), "", "a fleet session compiled on the laptop");
    let mine = gate(&host, None, &["build"]);
    assert_eq!(mine.status.code(), Some(0), "{}", String::from_utf8_lossy(&mine.stderr));
    assert_eq!(compiled(&host), "build\n");
}

// KEEP(contract): a fleet session's cargo that compiles nothing (metadata, fmt) and one with a local need keep running here; north#634
#[test]
fn a_fleet_sessions_cargo_that_compiles_nothing_or_needs_this_host_runs_here() {
    let host = host();
    let mut needy = host.command(env!("CARGO_BIN_EXE_north"));
    needy.env("RESIDENT_CAP_HOST", "laptop");
    needy.env("AGENT_CAPACITY_SELF_CGROUP", host.path("self-cgroup"));
    needy.args(["capacity", "gate", "cargo", "build"]).env("PATH", format!("{}:{}", host.path("real").display(), host.path("bin").display())).env("NORTH_FLEET_SESSION", "n634-x").env("AGENT_CAPACITY_NEED", "gpu");
    assert_eq!(needy.output_exclusive().unwrap().status.code(), Some(0));
    assert_eq!(gate(&host, Some("n634-x"), &["metadata"]).status.code(), Some(0));
    assert_eq!(gate(&host, Some("n634-x"), &["fmt"]).status.code(), Some(0));
    assert_eq!(compiled(&host), "build\nmetadata\nfmt\n");
}

// KEEP(contract): a fleet session outside any dev shell on a laptop with no cargo on PATH is refused for want of a build host, not sent into the dev shell to compile here; north#634
#[test]
fn a_fleet_sessions_cargo_with_no_real_cargo_on_path_is_refused_not_run_in_the_dev_shell() {
    let host = host();
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    command.env("RESIDENT_CAP_HOST", "laptop");
    command.args(["capacity", "gate", "cargo", "test"]).env("PATH", host.path("bin")).env("AGENT_CAPACITY_SELF_CGROUP", host.path("self-cgroup")).env("NORTH_FLEET_SESSION", "n634-x");
    let refused = command.output_exclusive().unwrap();
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(75), "{stderr}");
    assert!(stderr.contains("no build host took the job"), "{stderr}");
}
