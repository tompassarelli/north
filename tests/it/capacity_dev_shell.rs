use crate::capacity_host;

use north_core::foundation::shim;
use capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Output;

const FAKE_NIX: &str = "#!/bin/sh
[ \"$1\" = develop ] && [ \"$3\" = -c ] || exit 64
PATH=$2/shell-bin:$PATH
shift 3
exec \"$@\"
";

fn host_tool(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(name)).find(|path| path.is_file())
}

fn flake_host() -> Host {
    let host = Host::new("capacity-dev-shell-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL), ("nix", FAKE_NIX)]);
    for tool in ["sh", "env", "sed", "cut", "kill", "cat"] {
        if let Some(found) = host_tool(tool) {
            symlink(found, host.path("bin").join(tool)).unwrap();
        }
    }
    let project = host.path("project");
    fs::create_dir_all(project.join("shell-bin")).unwrap();
    fs::create_dir_all(project.join(".git")).unwrap();
    fs::create_dir_all(project.join("crates/game")).unwrap();
    fs::write(project.join("flake.nix"), "{ outputs = _: { devShells = { }; }; }\n").unwrap();
    shim::write_script(&project.join("shell-bin/cargo"), "echo \"cargo 1.0.0 from $NORTH_DEV_SHELL\"\n");
    fs::create_dir_all(host.path("plain/.git")).unwrap();
    host
}

fn leased(host: &Host, directory: &Path, class: &str, command: &[&str]) -> Output {
    host.command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "host", "run", "--class", class, "--owner", "dev-shell", "--timeout-seconds", "30", "--"])
        .args(command)
        .current_dir(directory)
        .env("PATH", host.path("bin"))
        .env_remove("IN_NIX_SHELL")
        .env_remove("NORTH_DEV_SHELL")
        .output()
        .unwrap()
}

#[test]
fn a_capacity_run_from_a_flake_project_finds_the_toolchain_its_dev_shell_provides() {
    let host = flake_host();
    let project = host.path("project");
    for class in ["heavy", "moderate"] {
        let output = leased(&host, &project.join("crates/game"), class, &["cargo", "--version"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{class}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {}", project.display()), "{class}: {stderr}");
    }
}

#[test]
fn a_leased_command_whose_program_is_not_found_exits_127_saying_not_found() {
    let host = flake_host();
    let project = host.path("project").join("crates/game");
    let plain = host.path("plain");
    for (directory, class) in [(&project, "heavy"), (&project, "critical"), (&plain, "moderate"), (&plain, "critical")] {
        let output = leased(&host, directory, class, &["no-such-program-465", "test"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(127), "{class} in {}: {stderr}", directory.display());
        assert!(stderr.contains("no-such-program-465: command not found"), "{class} in {}: {stderr}", directory.display());
    }
}

#[test]
fn a_shimmed_toolchain_this_host_lacks_exits_with_the_infrastructure_status_through_the_shim_and_the_gate() {
    let bin = tempfile::tempdir().unwrap();
    fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/cargo"), bin.path().join("cargo")).unwrap();
    for tool in ["sh", "readlink"] {
        if let Some(found) = host_tool(tool) {
            symlink(found, bin.path().join(tool)).unwrap();
        }
    }
    let shimmed = std::process::Command::new(bin.path().join("cargo")).arg("--version").env("PATH", bin.path()).output().unwrap();
    assert_eq!(shimmed.status.code(), Some(75), "{}", String::from_utf8_lossy(&shimmed.stderr));
    let gated = std::process::Command::new(env!("CARGO_BIN_EXE_north")).args(["capacity", "gate", "cargo", "--version"]).env("PATH", bin.path()).output().unwrap();
    assert_eq!(gated.status.code(), Some(75), "{}", String::from_utf8_lossy(&gated.stderr));
}

#[test]
fn the_gate_runs_a_toolchain_the_host_lacks_inside_the_project_dev_shell_that_provides_it() {
    let host = flake_host();
    let project = host.path("project");
    let output = host.command(env!("CARGO_BIN_EXE_north")).args(["capacity", "gate", "cargo", "--version"]).current_dir(project.join("crates/game")).env("PATH", host.path("bin")).env_remove("IN_NIX_SHELL").env_remove("NORTH_DEV_SHELL").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {}", project.display()), "{stderr}");
}
