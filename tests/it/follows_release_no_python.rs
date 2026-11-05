use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

#[test]
fn the_queue_read_succeeds_with_no_python3_on_path() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::create_dir_all(root.join("bin")).unwrap();
    symlink(env!("CARGO_BIN_EXE_north"), root.join("bin/north")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .arg("follows-release")
        .env_clear()
        .env("HOME", root)
        .env("NORTH_HOME", root)
        .env("PATH", root.join("bin"))
        .env("THREADS_DB", root.join("threads.db"))
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{:?}: {stderr}", output.status);
    assert!(!stderr.contains("python3"), "{stderr}");
}
