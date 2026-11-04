mod capacity_host;

use north_core::foundation::shim;
use capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use std::fs;
use std::os::unix::fs::symlink;

const FAKE_NIX: &str = "#!/bin/sh
[ \"$1\" = develop ] && [ \"$3\" = -c ] || exit 64
PATH=$2/shell-bin:$PATH
shift 3
exec \"$@\"
";

fn host_tool(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(name)).find(|path| path.is_file())
}

#[test]
fn a_capacity_run_from_a_flake_project_finds_the_toolchain_its_dev_shell_provides() {
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
    let output = host
        .command(env!("CARGO_BIN_EXE_north"))
        .args(["capacity", "host", "run", "--class", "moderate", "--owner", "dev-shell", "--timeout-seconds", "30", "--", "cargo", "--version"])
        .current_dir(project.join("crates/game"))
        .env("PATH", host.path("bin"))
        .env_remove("IN_NIX_SHELL")
        .env_remove("NORTH_DEV_SHELL")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("cargo 1.0.0 from {}", project.display()), "{stderr}");
}
