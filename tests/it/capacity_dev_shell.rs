use crate::capacity_host;

use north_core::foundation::shim;
use capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Output;

const FAKE_NIX: &str = "#!/bin/sh
case \"$1\" in
  eval) echo \"${SHELL_DRV:-/nix/store/0000-default-shell.drv}\" ;;
  print-dev-env)
    flake=$2
    while [ \"$1\" != --profile ]; do shift; done
    echo \"$2\" >>\"${PREPARES:-/dev/null}\"
    ln -sfn \"$flake\" \"$2\" ;;
  develop)
    profile=$2
    shift 2
    while [ \"$1\" != -c ]; do shift; done
    shift
    echo entered >>\"${ENTRIES:-/dev/null}\"
    PATH=$profile/shell-bin:$PATH
    exec \"$@\" ;;
  *) exit 64 ;;
esac
";

const DEFAULT_SHELL: &str = "/nix/store/0000-default-shell.drv";

fn host_tool(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(name)).find(|path| path.is_file())
}

fn flake_host() -> Host {
    let host = Host::new("capacity-dev-shell-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL), ("nix", FAKE_NIX)]);
    for tool in ["sh", "env", "sed", "cut", "kill", "cat", "ln", "readlink", "dirname", "bash"] {
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
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {DEFAULT_SHELL}"), "{class}: {stderr}");
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
    shim::copy_executable(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/cargo"), bin.path().join("cargo"));
    for tool in ["sh", "readlink"] {
        if let Some(found) = host_tool(tool) {
            symlink(found, bin.path().join(tool)).unwrap();
        }
    }
    std::fs::create_dir(bin.path().join(".git")).unwrap();
    let shimmed = std::process::Command::new(bin.path().join("cargo")).arg("--version").env("PATH", bin.path()).output().unwrap();
    assert_eq!(shimmed.status.code(), Some(75), "{}", String::from_utf8_lossy(&shimmed.stderr));
    let gated = std::process::Command::new(env!("CARGO_BIN_EXE_north")).args(["capacity", "gate", "cargo", "--version"]).current_dir(bin.path()).env("PATH", bin.path()).output().unwrap();
    assert_eq!(gated.status.code(), Some(75), "{}", String::from_utf8_lossy(&gated.stderr));
}

#[test]
fn the_gate_runs_a_toolchain_the_host_lacks_inside_the_project_dev_shell_that_provides_it() {
    let host = flake_host();
    let project = host.path("project");
    let output = host.command(env!("CARGO_BIN_EXE_north")).args(["capacity", "gate", "cargo", "--version"]).current_dir(project.join("crates/game")).env("PATH", host.path("bin")).env_remove("IN_NIX_SHELL").env_remove("NORTH_DEV_SHELL").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {DEFAULT_SHELL}"), "{stderr}");
}

#[test]
fn a_check_script_that_enters_the_dev_shell_inside_an_entered_shell_runs_once_with_the_real_cargo() {
    let host = flake_host();
    symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north")).unwrap();
    let project = host.path("project");
    let (entries, builds) = (host.path("entries"), host.path("builds"));
    shim::write_script(&project.join("shell-bin/cargo"), &format!("echo \"real cargo $*\" >>{}\n", builds.display()));
    shim::write_script(&host.path("bin/cargo"), "echo \"gate shim\" >&2\nexit 75\n");
    fs::write(project.join("check.sh"), "north dev-shell run -- cargo build\n").unwrap();
    let output = host
        .command(env!("CARGO_BIN_EXE_north"))
        .args(["dev-shell", "run", "--dir"])
        .arg(&project)
        .args(["--", "sh", "check.sh"])
        .current_dir(&project)
        .env("PATH", host.path("bin"))
        .env("ENTRIES", &entries)
        .env("NORTH_DEV_SHELL_TRACE", "1")
        .env_remove("IN_NIX_SHELL")
        .env_remove("NORTH_DEV_SHELL")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(fs::read_to_string(&entries).unwrap(), "entered\n", "{stderr}");
    assert_eq!(fs::read_to_string(&builds).unwrap(), "real cargo build\n", "{stderr}");
    assert_eq!(stderr.matches("north dev-shell: enter ").count(), 1, "{stderr}");
    assert_eq!(stderr.matches("north dev-shell: direct ").count(), 1, "{stderr}");
}

#[test]
fn a_flake_without_a_dev_shell_runs_the_command_bare_saying_so() {
    let host = Host::new("dev-shell-bare-", &[("nix", "#!/bin/sh\nexit 1\n")]);
    for tool in ["sh", "echo"] {
        if let Some(found) = host_tool(tool) {
            symlink(found, host.path("bin").join(tool)).unwrap();
        }
    }
    let project = host.path("project");
    fs::create_dir_all(project.join(".git")).unwrap();
    fs::write(project.join("flake.nix"), "{ outputs = _: { }; }\n").unwrap();
    let output = host.command(env!("CARGO_BIN_EXE_north")).args(["dev-shell", "run", "--", "echo", "bare"]).current_dir(&project).env("PATH", host.path("bin")).env_remove("NORTH_DEV_SHELL").output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "bare\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("has no devShells."), "{}", String::from_utf8_lossy(&output.stderr));
}

fn north_in(host: &Host, directory: &Path, arguments: &[&str]) -> std::process::Command {
    let mut command = host.command(env!("CARGO_BIN_EXE_north"));
    command.args(arguments).current_dir(directory).env("PATH", host.path("bin")).env_remove("IN_NIX_SHELL").env_remove("NORTH_DEV_SHELL").env_remove("AGENT_CAPACITY_LEASE");
    command
}

fn roots(host: &Host) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(host.path("state/north/dev-shells")).map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| !name.starts_with('.')).collect()).unwrap_or_default();
    names.sort();
    names
}

#[test]
fn exec_with_a_dev_shell_enters_it_and_finds_the_toolchain_from_a_plain_shell() {
    let host = flake_host();
    let project = host.path("project");
    let output = north_in(&host, &host.path("plain"), &["capacity", "exec", "--dev-shell"]).arg(&project).args(["--", "cargo", "--version"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("cargo 1."), "{stderr}");
    let bare = north_in(&host, &host.path("plain"), &["capacity", "exec", "--", "cargo", "--version"]).output().unwrap();
    assert_eq!(bare.status.code(), Some(127), "{}", String::from_utf8_lossy(&bare.stderr));
}

#[test]
fn a_stray_in_nix_shell_does_not_count_as_inside_the_project_shell() {
    let host = flake_host();
    let directory = host.path("project").join("crates/game");
    let output = north_in(&host, &directory, &["capacity", "host", "run", "--class", "heavy", "--owner", "dev-shell", "--timeout-seconds", "30", "--", "cargo", "--version"]).env("IN_NIX_SHELL", "impure").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {DEFAULT_SHELL}"), "{stderr}");
    let gated = north_in(&host, &directory, &["capacity", "gate", "cargo", "--version"]).env("IN_NIX_SHELL", "impure").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&gated.stdout).trim(), format!("cargo 1.0.0 from {DEFAULT_SHELL}"), "{}", String::from_utf8_lossy(&gated.stderr));
}

#[test]
fn a_flake_that_only_mentions_dev_shells_is_no_project_shell() {
    let host = flake_host();
    shim::write_script(&host.path("bin/nix"), "echo \"error: flake does not provide attribute 'devShells'\" >&2\nexit 1\n");
    fs::write(host.path("project/flake.nix"), "# devShells come later\n{ outputs = _: { }; }\n").unwrap();
    let gated = north_in(&host, &host.path("project/crates/game"), &["capacity", "gate", "cargo", "--version"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&gated.stderr);
    assert_eq!(gated.status.code(), Some(75), "{stderr}");
    assert!(stderr.contains("command not found behind the north capacity gate"), "{stderr}");
}

#[test]
fn land_shell_dev_with_the_root_removed_realises_it_once_then_enters_offline() {
    let host = flake_host();
    symlink(env!("CARGO_BIN_EXE_north"), host.path("bin/north")).unwrap();
    let project = host.path("project");
    fs::create_dir_all(project.join("bin")).unwrap();
    shim::copy_executable(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/land-shell"), project.join("bin/land-shell"));
    shim::write_script(&project.join("bin/depbox"), "[ \"$1 $2\" = \"run --\" ] || exit 64\nshift 2\nDEPBOX=run:test exec \"$@\"\n");
    let prepares = host.path("prepares");
    let land = |expected: usize| {
        let output = host.command(project.join("bin/land-shell")).args(["--dev", "--", "cargo", "--version"]).current_dir(&project).env("PATH", host.path("bin")).env_remove("IN_NIX_SHELL").env_remove("NORTH_DEV_SHELL").env_remove("AGENT_CAPACITY_LEASE").env_remove("DEPBOX").env_remove("GITHUB_ACTIONS").env("PREPARES", &prepares).env("ENTRIES", host.path("entries")).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stderr}");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {DEFAULT_SHELL}"), "{stderr}");
        assert_eq!(fs::read_to_string(&prepares).unwrap_or_default().lines().count(), expected, "{stderr}");
    };
    land(1);
    assert_eq!(roots(&host), ["0000-default-shell"]);
    land(1);
    fs::remove_dir_all(host.path("state/north/dev-shells/0000-default-shell")).unwrap();
    land(2);
    assert_eq!(roots(&host), ["0000-default-shell"]);
}

#[test]
fn an_edit_that_changes_the_shell_derivation_gets_a_new_root_and_drops_the_old() {
    let host = flake_host();
    let project = host.path("project");
    let prepare = |derivation: &str| north_in(&host, &project, &["dev-shell", "run", "--prepare"]).env("SHELL_DRV", derivation).output().unwrap();
    assert!(prepare(DEFAULT_SHELL).status.success());
    assert_eq!(roots(&host), ["0000-default-shell"]);
    let edited = prepare("/nix/store/1111-edited-shell.drv");
    assert!(edited.status.success(), "{}", String::from_utf8_lossy(&edited.stderr));
    assert_eq!(roots(&host), ["1111-edited-shell"]);
}
