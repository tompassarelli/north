mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, Wrapper, scope_systemd_run};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

const LEASED_HOST: &str = "[hosts.gex45]\nlease_builds = true\n";

fn host(prefix: &str, orchestration: &str) -> Host {
    let host = Host::new(prefix, &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    fs::write(host.path("orchestration.toml"), orchestration).unwrap();
    fs::write(host.path("cgroup-self"), "0::/user.slice/user-1000.slice/session-1.scope\n").unwrap();
    let real = host.path("toolchain/cargo");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::write(&real, format!("#!/bin/sh\necho \"$CARGO_BUILD_JOBS $*\" >{}\n", host.path("built").display())).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
    let shim = host.path("shims/cargo");
    fs::create_dir_all(shim.parent().unwrap()).unwrap();
    fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/cargo"), &shim).unwrap();
    host
}

fn cargo(host: &Host, arguments: &[&str]) -> Command {
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    let path = format!("{}:{}:{}:{}", host.path("shims").display(), host.path("toolchain").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default());
    command.args(["capacity", "gate"]).arg(host.path("shims/cargo")).args(arguments).env("PATH", path).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env_remove("AGENT_CAPACITY_LEASE").env_remove("NORTH_FLEET_SESSION").env_remove("CARGO_BUILD_JOBS");
    command
}

#[test]
fn a_cargo_build_outside_a_lease_on_a_lease_builds_host_waits_for_a_heavy_lease_then_builds_with_the_lease_cpus() {
    let host = host("capacity-gate-wait-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    let mut build = Wrapper(cargo(&host, &["build", "--release"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    host.until("the build's heavy ticket", |host| host.queued("heavy") == 1);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!host.path("built").exists(), "the build ran before its lease");
    assert!(build.exited().is_none());
    fs::write(host.path("pressure"), "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    host.until("the leased build", |host| host.path("built").exists());
    assert!(build.0.wait().unwrap().success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), "6 build --release\n");
}

#[test]
fn a_cargo_build_on_a_host_without_lease_builds_and_a_light_cargo_command_run_at_once() {
    let host = host("capacity-gate-open-", "[hosts.gex45]\nresident_cap = 24\n");
    assert!(cargo(&host, &["build"]).status().unwrap().success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), " build\n");
    let host = self::host("capacity-gate-light-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    assert!(cargo(&host, &["fmt", "--check"]).status().unwrap().success());
    assert_eq!((fs::read_to_string(host.path("built")).unwrap(), host.queued("heavy")), (" fmt --check\n".to_owned(), 0));
}

#[test]
fn the_shim_runs_the_next_cargo_on_path_when_north_has_no_gate() {
    let host = host("capacity-gate-fallback-", LEASED_HOST);
    let stale = host.path("stale/north");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, "#!/bin/sh\necho usage >&2\nexit 2\n").unwrap();
    fs::set_permissions(&stale, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}:{}:/run/current-system/sw/bin:/usr/bin:/bin", host.path("stale").display(), host.path("shims").display(), host.path("toolchain").display());
    let status = Command::new(host.path("shims/cargo")).arg("test").env("PATH", path).env_remove("CARGO_BUILD_JOBS").status().unwrap();
    assert!(status.success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), " test\n");
}

#[test]
fn depbox_outside_a_lease_on_a_lease_builds_host_waits_for_a_heavy_lease_before_it_starts() {
    let host = host("capacity-gate-depbox-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north")).unwrap();
    let mut depbox = host.command(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/depbox"));
    depbox.args(["run", "--", "cargo", "test"]).current_dir(host.path("")).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env_remove("AGENT_CAPACITY_LEASE");
    let mut waiting = Wrapper(depbox.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    host.until("depbox's heavy ticket", |host| host.queued("heavy") == 1);
    assert!(waiting.exited().is_none(), "depbox started before its lease");
}

#[test]
fn depbox_under_an_inherited_live_lease_outside_its_scope_runs_once_nested_instead_of_asking_again() {
    let host = host("capacity-gate-inherited-", LEASED_HOST);
    let _holder = Wrapper(host.helper(&["run", "--class", "heavy", "--owner", "fixture:/holder", "--timeout-seconds", "60", "--", "sleep", "30"]).stdout(Stdio::null()).spawn().unwrap());
    host.until("the holder's heavy lease", |host| host.leases("heavy") == 1);
    let lease = fs::read_dir(host.path("agent-capacity-v1/leases")).unwrap().find_map(|entry| Some(entry.ok()?.file_name().to_str()?.strip_suffix(".json")?.to_owned())).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north")).unwrap();
    let mut depbox = host.command(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/depbox"));
    depbox.args(["run", "--", "cargo", "test"]).current_dir(host.path("")).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env("AGENT_CAPACITY_LEASE", &lease);
    let mut nested = Wrapper(depbox.process_group(0).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while nested.exited().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let finished = nested.exited().is_some();
    if !finished {
        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{}", nested.0.id())]).status();
    }
    assert!(finished, "depbox kept asking the gate under its inherited lease");
    assert!(nested.stderr().contains("depbox: run inside a git checkout"));
}
