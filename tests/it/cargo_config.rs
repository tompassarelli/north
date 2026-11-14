#[test]
fn dev_and_test_builds_run_rustc_without_the_parallel_front_end() {
    let config = std::fs::read_to_string(crate::support::manifest_dir().join(".cargo/config.toml")).unwrap();
    let threaded: Vec<&str> = config.lines().filter(|line| line.contains("-Zthreads")).collect();
    assert!(threaded.is_empty(), "the workspace cargo config turns on rustc's parallel front end, which deadlocks intermittently: {threaded:?}");
}

#[test]
fn a_cargo_config_planted_in_a_worker_writable_home_steers_no_capacity_run() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let (home, project, wrapper, marker) = (scratch.path().join("home"), scratch.path().join("project"), scratch.path().join("wrapper"), scratch.path().join("wrapped"));
    std::fs::create_dir_all(home.join(".cargo")).unwrap();
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(&wrapper, format!("#!/bin/sh\n: > {}\nexec \"$@\"\n", marker.display())).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(home.join(".cargo/config.toml"), format!("[build]\nrustc-wrapper = \"{}\"\n", wrapper.display())).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"planted\"\nversion = \"0.0.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::write(project.join("src/lib.rs"), "").unwrap();
    let cargo = std::env::var_os("CARGO").map(std::path::PathBuf::from).or_else(|| std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|directory| directory.join("cargo")).find(|path| path.is_file() && !gate_shim(path))).expect("a real cargo on PATH");
    let output = std::process::Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["capacity", "exec", "--"])
        .arg(&cargo)
        .args(["build", "--offline", "--quiet"])
        .current_dir(&project)
        .env("PATH", cargo.parent().unwrap())
        .env("HOME", &home)
        .env("CARGO_TARGET_DIR", scratch.path().join("target"))
        .env_remove("CARGO_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!marker.exists(), "a capacity run's cargo ran the build wrapper a worker planted in ~/.cargo/config.toml, so code a worker wrote ran outside its sandbox: {stderr}");
    assert!(!output.status.success() && stderr.contains("config.toml"), "a run with a planted ~/.cargo/config.toml went on instead of refusing it: {stderr}");
}

fn gate_shim(path: &std::path::Path) -> bool {
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|file| std::io::Read::read_to_end(&mut std::io::Read::take(file, 512), &mut head)).is_ok() && head.windows(24).any(|window| window == b"north capacity gate shim")
}
