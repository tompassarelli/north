mod capacity_host;

use capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Stdio;

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_file() && !gate_shim(path))
}

fn gate_shim(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 256];
    let read = fs::File::open(path).and_then(|mut file| file.read(&mut head)).unwrap_or(0);
    head[..read].starts_with(b"#!") && head[..read].windows(13).any(|window| window == b"capacity gate")
}

#[test]
fn admission_spawns_the_promoted_north_when_path_has_no_north() {
    let host = Host::new("capacity-north-path-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
    for tool in ["bun", "timeout", "env", "sh", "true", "cut", "sed", "cat", "kill", "sleep", "mkdir", "rm", "dirname", "basename"] {
        if let Some(found) = on_path(tool) {
            symlink(found, host.path("bin").join(tool)).unwrap();
        }
    }
    let promoted = host.path("home/.local/state/north/current");
    fs::create_dir_all(&promoted).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), promoted.join("north")).unwrap();
    let mut command = host.command("bun");
    command
        .arg(Host::bundle())
        .args(["run", "--class", "moderate", "--owner", "north-path", "--timeout-seconds", "30", "--", "true"])
        .env("PATH", host.path("bin"))
        .env("HOME", host.path("home"))
        .env_remove("AGENT_CAPACITY_NORTH")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = command.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains("ENOENT"), "{stderr}");
}

#[test]
fn status_works_when_the_north_on_path_predates_the_script() {
    let stale = "#!/bin/sh\necho 'usage: north capacity [probe|run|session|release]' >&2\nexit 64\n";
    let host = Host::new("capacity-stale-north-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL), ("north", stale)]);
    for tool in ["bun", "timeout", "env", "sh", "true", "cut", "sed", "cat", "kill", "sleep", "mkdir", "rm", "dirname", "basename"] {
        if let Some(found) = on_path(tool) {
            symlink(found, host.path("bin").join(tool)).unwrap();
        }
    }
    let promoted = host.path("home/.local/state/north/current");
    fs::create_dir_all(&promoted).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), promoted.join("north")).unwrap();
    let mut command = host.command("bun");
    command
        .arg(Host::bundle())
        .arg("status")
        .env("PATH", host.path("bin"))
        .env("HOME", host.path("home"))
        .env_remove("AGENT_CAPACITY_NORTH")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains("usage: north capacity"), "{stderr}");
}
