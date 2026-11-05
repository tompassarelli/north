use crate::capacity_host;

use north_core::foundation::shim;
use capacity_host::{Exclusive, Host, SCOPE_SYSTEMCTL, Wrapper, exclusive, scope_systemd_run};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

const LEASED_HOST: &str = "[hosts.gex45]\nlease_builds = true\n";

fn host(prefix: &str, orchestration: &str) -> Host {
    let host = Host::new(prefix, &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    let _writing = exclusive();
    fs::write(host.path("orchestration.toml"), orchestration).unwrap();
    fs::write(host.path("cgroup-self"), "0::/user.slice/user-1000.slice/session-1.scope\n").unwrap();
    let real = host.path("toolchain/cargo");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    shim::write_script(&real, &format!("echo \"$CARGO_BUILD_JOBS $*\" >{}\n", host.path("built").display()));
    let shim = host.path("shims/cargo");
    fs::create_dir_all(shim.parent().unwrap()).unwrap();
    shim::copy_executable(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/cargo"), &shim);
    shim::copy_executable(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/nix"), host.path("shims/nix"));
    let nix = host.path("toolchain/nix");
    shim::write_script(&nix, &format!("while [ \"$1\" != --command ]; do shift; done\nshift\nPATH={}:$PATH exec \"$@\"\n", real.parent().unwrap().display()));
    host
}

fn within_20_seconds(mut child: std::process::Child) -> Option<std::process::ExitStatus> {
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if started.elapsed() > std::time::Duration::from_secs(20) {
            let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).status();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn cargo(host: &Host, arguments: &[&str]) -> Command {
    gated(host, "cargo", arguments)
}

fn gated(host: &Host, shim: &str, arguments: &[&str]) -> Command {
    let _ = std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north"));
    let mut command = host.command(host.path("shims").join(shim));
    let path = format!("{}:{}:{}:{}", host.path("shims").display(), host.path("toolchain").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default());
    command.args(arguments).env("PATH", path).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env_remove("AGENT_CAPACITY_LEASE").env_remove("NORTH_FLEET_SESSION").env_remove("CARGO_BUILD_JOBS");
    command
}

#[test]
fn a_cargo_build_outside_a_lease_on_a_lease_builds_host_waits_for_a_heavy_lease_then_builds_with_the_lease_cpus() {
    let host = host("capacity-gate-wait-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    let mut build = Wrapper(cargo(&host, &["build", "--release"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn_exclusive().unwrap());
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
fn a_cargo_build_inside_nix_develop_over_ssh_on_a_lease_builds_host_waits_for_a_heavy_lease_though_the_dev_shell_cargo_comes_first() {
    let host = host("capacity-gate-nix-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    let mut build = Wrapper(gated(&host, "nix", &["develop", "--command", "cargo", "build", "--release", "-p", "wisp-window"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn_exclusive().unwrap());
    host.until("the build's heavy ticket", |host| host.queued("heavy") == 1);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!host.path("built").exists(), "the build ran before its lease");
    assert!(build.exited().is_none());
    fs::write(host.path("pressure"), "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    host.until("the leased build", |host| host.path("built").exists());
    assert!(build.0.wait().unwrap().success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), "6 build --release -p wisp-window\n");
    let host = self::host("capacity-gate-nix-shell-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    assert!(gated(&host, "nix", &["develop", "--command", "cargo", "fmt"]).status_exclusive().unwrap().success());
    assert_eq!((fs::read_to_string(host.path("built")).unwrap(), host.queued("heavy")), (" fmt\n".to_owned(), 0));
}

#[test]
fn a_cargo_build_on_a_host_without_lease_builds_and_a_light_cargo_command_run_at_once() {
    let host = host("capacity-gate-open-", "[hosts.gex45]\nresident_cap = 24\n");
    assert!(cargo(&host, &["build"]).status_exclusive().unwrap().success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), " build\n");
    let host = self::host("capacity-gate-light-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    assert!(cargo(&host, &["fmt", "--check"]).status_exclusive().unwrap().success());
    assert_eq!((fs::read_to_string(host.path("built")).unwrap(), host.queued("heavy")), (" fmt --check\n".to_owned(), 0));
}

#[test]
fn the_shim_runs_the_next_cargo_on_path_when_north_has_no_gate() {
    let host = host("capacity-gate-fallback-", LEASED_HOST);
    let stale = host.path("stale/north");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    shim::write_script(&stale, "echo usage >&2\nexit 2\n");
    let path = format!("{}:{}:{}:/run/current-system/sw/bin:/usr/bin:/bin", host.path("stale").display(), host.path("shims").display(), host.path("toolchain").display());
    let status = Command::new(host.path("shims/cargo")).arg("test").env("PATH", path).env_remove("CARGO_BUILD_JOBS").status_exclusive().unwrap();
    assert!(status.success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), " test\n");
}

#[test]
fn depbox_outside_a_lease_on_a_lease_builds_host_waits_for_a_heavy_lease_before_it_starts() {
    let host = host("capacity-gate-depbox-", LEASED_HOST);
    fs::write(host.path("pressure"), "some avg10=90.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north")).unwrap();
    let mut depbox = host.command(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/depbox"));
    depbox.env_remove("DEPBOX");
    depbox.args(["run", "--", "cargo", "test"]).current_dir(host.path("")).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env_remove("AGENT_CAPACITY_LEASE");
    let mut waiting = Wrapper(depbox.stdout(Stdio::null()).stderr(Stdio::null()).spawn_exclusive().unwrap());
    host.until("depbox's heavy ticket", |host| host.queued("heavy") == 1);
    assert!(waiting.exited().is_none(), "depbox started before its lease");
}

#[test]
fn depbox_under_an_inherited_live_lease_outside_its_scope_runs_once_nested_instead_of_asking_again() {
    let host = host("capacity-gate-inherited-", LEASED_HOST);
    let _holder = Wrapper(host.helper(&["run", "--class", "heavy", "--owner", "fixture:/holder", "--timeout-seconds", "60", "--", "sleep", "30"]).stdout(Stdio::null()).spawn_exclusive().unwrap());
    host.until("the holder's heavy lease", |host| host.leases("heavy") == 1);
    let lease = fs::read_dir(host.path("agent-capacity-v1/leases")).unwrap().find_map(|entry| Some(entry.ok()?.file_name().to_str()?.strip_suffix(".json")?.to_owned())).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north")).unwrap();
    let mut depbox = host.command(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/depbox"));
    depbox.env_remove("DEPBOX");
    depbox.args(["run", "--", "cargo", "test"]).current_dir(host.path("")).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env("AGENT_CAPACITY_LEASE", &lease);
    let mut nested = Wrapper(depbox.process_group(0).stdout(Stdio::null()).stderr(Stdio::piped()).spawn_exclusive().unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while nested.exited().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let finished = nested.exited().is_some();
    if !finished {
        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{}", nested.0.id())]).status_exclusive();
    }
    assert!(finished, "depbox kept asking the gate under its inherited lease");
    let stderr = nested.stderr();
    assert!(stderr.contains("depbox: run inside a git checkout"), "{stderr}");
}

#[test]
fn a_stub_that_execs_the_gate_shim_reaches_the_real_program_instead_of_looping() {
    let host = host("capacity-gate-loop-", "[hosts.gex45]\nresident_cap = 24\n");
    shim::write_script(&host.path("toolchain/bun"), &format!("echo \"real $*\" >{}\n", host.path("ran").display()));
    shim::copy_executable(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/bun"), host.path("shims/bun"));
    let stub = host.path("stubs/bun");
    fs::create_dir_all(stub.parent().unwrap()).unwrap();
    shim::write_script(&stub, &format!("exec {} \"$@\"\n", host.path("shims/bun").display()));
    let mut command = gated(&host, "bun", &["test"]);
    let path = format!("{}:{}:{}:{}:{}", host.path("stubs").display(), host.path("shims").display(), host.path("toolchain").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default());
    let child = command.env("PATH", path).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn_exclusive().unwrap();
    let status = within_20_seconds(child);
    assert!(status.is_some_and(|status| status.success()), "a stub that execs the gate shim looped through the gate instead of reaching the real program");
    assert_eq!(fs::read_to_string(host.path("ran")).unwrap(), "real test\n");
}

#[test]
fn a_check_script_that_runs_nix_develop_inside_a_dev_shell_reaches_the_real_nix_behind_the_gate() {
    let host = host("capacity-gate-nested-nix-", "[hosts.gex45]\nresident_cap = 24\n");
    let check = host.path("check.sh");
    fs::write(&check, "exec nix develop --command cargo fmt\n").unwrap();
    let output = gated(&host, "nix", &["develop", "--command", "bash", &check.to_string_lossy()]).output_exclusive().unwrap();
    assert!(output.status.success(), "the nested nix did not resolve behind the gate: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), " fmt\n");
}

#[test]
fn a_stub_that_names_the_dev_shell_shim_ahead_of_the_real_bun_is_skipped_as_a_shim_before_the_loop_guard_fires() {
    let host = host("capacity-gate-shim-identity-", "[hosts.gex45]\nresident_cap = 24\n");
    shim::write_script(&host.path("toolchain/bun"), &format!("printf '%s\\n' \"$NORTH_CAPACITY_GATE_EXECUTED\" >{}\n", host.path("ran").display()));
    let shims = host.path("store/0123456789abcdefghijklmnopqrstuv-capacity-gate-shims/bin");
    fs::create_dir_all(&shims).unwrap();
    shim::copy_executable(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/bun"), shims.join("bun"));
    let stub = host.path("stubs/bun");
    fs::create_dir_all(stub.parent().unwrap()).unwrap();
    shim::write_script(&stub, &format!("[ \"$*\" = \"install --frozen-lockfile\" ] || exec {} \"$@\"\n", shims.join("bun").display()));
    let path = format!("{}:{}:{}:{}:{}", host.path("stubs").display(), shims.display(), host.path("toolchain").display(), host.path("bin").display(), std::env::var("PATH").unwrap_or_default());
    let _ = std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north"));
    let child = host.command(&stub).arg("test").env("PATH", path).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self")).env_remove("NORTH_CAPACITY_GATE_EXECUTED").stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn_exclusive().unwrap();
    let status = within_20_seconds(child);
    assert!(status.is_some_and(|status| status.success()), "the gate looped through the stub instead of reaching the real bun");
    let chain = fs::read_to_string(host.path("ran")).unwrap();
    let executed: Vec<&str> = chain.trim_end().split('\t').skip(1).collect();
    assert_eq!(executed.len(), 1, "the gate executed the stub before the real bun, so only the loop guard stopped it: {chain}");
    assert!(executed[0].ends_with(&format!(" {}", fs::canonicalize(host.path("toolchain/bun")).unwrap().display())), "{chain}");
}

fn without_cargo(host: &Host, dev_shell_tools: &str) -> Command {
    fs::remove_file(host.path("toolchain/cargo")).unwrap();
    let tools = host.path(dev_shell_tools);
    fs::create_dir_all(&tools).unwrap();
    shim::write_script(&host.path("toolchain/nix"), &format!(
        "case $1 in eval) echo fixture-shell.drv; exit 0;; print-dev-env) while [ \"$1\" != --profile ]; do shift; done; mkdir -p \"$2\"; exit 0;; esac\nwhile [ \"$1\" != -c ]; do shift; done\nshift\nPATH={}:{}:$PATH exec \"$@\"\n",
        host.path("shims").display(),
        tools.display()
    ));
    let project = host.path("project");
    fs::create_dir_all(project.join(".git")).unwrap();
    fs::write(project.join("flake.nix"), "{ outputs = _: { devShells.x86_64-linux.default = null; }; }\n").unwrap();
    let _ = std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north"));
    let mut command = host.command(host.path("shims/cargo"));
    let system = crate::support::test_env::minimal_path(&host.path("system"), &["sh", "env", "cut", "kill", "sed", "mkdir", "cat"]);
    let path = format!("{}:{}:{}:{}", host.path("shims").display(), host.path("toolchain").display(), host.path("bin").display(), system.display());
    command.arg("--version").env("PATH", path).env("RESIDENT_CAP_HOST", "gex45").env("AGENT_CAPACITY_SELF_CGROUP", host.path("cgroup-self"));
    for name in ["NORTH_DEV_SHELL", "NORTH_CAPACITY_GATE_DEV_SHELL", "NORTH_CAPACITY_GATE_EXECUTED", "IN_NIX_SHELL", "CARGO_BUILD_JOBS"] {
        command.env_remove(name);
    }
    command
}

#[test]
fn with_no_real_cargo_the_gate_runs_it_through_north_dev_shell_run_in_a_project_with_a_dev_shell_and_names_it_elsewhere() {
    let host = host("capacity-gate-dev-shell-", LEASED_HOST);
    let mut command = without_cargo(&host, "dev-shell-tools");
    shim::write_script(&host.path("dev-shell-tools/cargo"), "echo \"cargo 1.99.0 (fixture) $NORTH_DEV_SHELL\"\n");
    let entered = command.current_dir(host.path("project")).output_exclusive().unwrap();
    assert!(entered.status.success(), "{}", String::from_utf8_lossy(&entered.stderr));
    assert_eq!(String::from_utf8_lossy(&entered.stdout), "cargo 1.99.0 (fixture) fixture-shell.drv\n");
    let bare = command.current_dir(host.path("")).output_exclusive().unwrap();
    assert_eq!(bare.status.code(), Some(75), "{}", String::from_utf8_lossy(&bare.stderr));
    assert!(String::from_utf8_lossy(&bare.stderr).starts_with("cargo: command not found behind the north capacity gate"), "{}", String::from_utf8_lossy(&bare.stderr));
}

#[test]
fn a_project_dev_shell_without_cargo_makes_the_gate_name_it_once_instead_of_entering_again() {
    let host = host("capacity-gate-dev-shell-empty-", LEASED_HOST);
    let mut command = without_cargo(&host, "empty-dev-shell-tools");
    let child = command.current_dir(host.path("project")).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn_exclusive().unwrap();
    let status = within_20_seconds(child);
    assert_eq!(status.and_then(|status| status.code()), Some(75), "the gate kept entering a dev shell that has no cargo");
}

#[test]
fn a_nix_build_outside_a_lease_on_a_lease_builds_host_builds_with_the_lease_cpus_as_cores_and_one_job() {
    let host = host("capacity-gate-nix-build-", LEASED_HOST);
    shim::write_script(&host.path("toolchain/nix"), &format!("echo \"$*\" >{}\n", host.path("built").display()));
    assert!(gated(&host, "nix", &["build", ".#shellcheck", "--rebuild"]).stdout(Stdio::null()).stderr(Stdio::null()).status_exclusive().unwrap().success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), "--cores 6 --max-jobs 1 build .#shellcheck --rebuild\n");
    fs::write(host.path("cgroup-self"), "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/agent-capacity-0123456789abcdef0123456789abcdef.scope\n").unwrap();
    assert!(gated(&host, "nix", &["build", ".#shellcheck"]).env("CARGO_BUILD_JOBS", "4").stdout(Stdio::null()).stderr(Stdio::null()).status_exclusive().unwrap().success());
    assert_eq!(fs::read_to_string(host.path("built")).unwrap(), "--cores 4 --max-jobs 1 build .#shellcheck\n", "a nix build already inside a lease ran without the lease's cores");
}
