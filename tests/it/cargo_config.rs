#[test]
fn dev_and_test_builds_run_rustc_without_the_parallel_front_end() {
    let config = std::fs::read_to_string(crate::support::manifest_dir().join(".cargo/config.toml")).unwrap();
    let threaded: Vec<&str> = config.lines().filter(|line| line.contains("-Zthreads")).collect();
    assert!(threaded.is_empty(), "the workspace cargo config turns on rustc's parallel front end, which deadlocks intermittently: {threaded:?}");
}

#[test]
fn a_cargo_config_or_subcommand_planted_where_a_worker_writes_steers_no_capacity_run() {
    let wrapper = |marker: &std::path::Path| format!("[build]\nrustc-wrapper = \"{}\"\n", marker.with_extension("sh").display());
    let build: &[&str] = &["build", "--offline", "--quiet"];
    refuses_planted("home/.cargo/config.toml", "home/code/proj/worktrees/lane", "~/.cargo/config.toml naming a build wrapper", wrapper, build);
    refuses_planted("home/code/proj/worktrees/.cargo/config.toml", "home/code/proj/worktrees/lane", "a build wrapper in a project's worktrees/.cargo/config.toml, above a lane", wrapper, build);
    refuses_planted("home/.cache/land-offload/.cargo/config.toml", "home/.cache/land-offload/code-proj-main-1", "a build wrapper in ~/.cache/land-offload/.cargo/config.toml, above an offload slot", wrapper, build);
    refuses_planted("home/.cargo/bin/cargo-planted", "home/code/proj/worktrees/lane", "~/.cargo/bin/cargo-planted, which cargo searches before PATH", |marker| format!("#!/bin/sh\n: > {}\n", marker.display()), &["planted"]);
}

fn refuses_planted(planted: &str, lane: &str, what: &str, content: impl Fn(&std::path::Path) -> String, arguments: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    let cargo = std::env::var_os("CARGO").map(std::path::PathBuf::from).or_else(|| std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|directory| directory.join("cargo")).find(|path| path.is_file() && !gate_shim(path))).expect("a real cargo on PATH");
    let scratch = tempfile::tempdir().unwrap();
    let (home, lane, marker, planted) = (scratch.path().join("home"), scratch.path().join(lane), scratch.path().join("ran"), scratch.path().join(planted));
    std::fs::create_dir_all(home.join(".cargo")).unwrap();
    std::fs::create_dir_all(lane.join(".git")).unwrap();
    std::fs::create_dir_all(lane.join("src")).unwrap();
    std::fs::create_dir_all(planted.parent().unwrap()).unwrap();
    std::fs::write(marker.with_extension("sh"), format!("#!/bin/sh\n: > {}\nexec \"$@\"\n", marker.display())).unwrap();
    std::fs::write(&planted, content(&marker)).unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(marker.with_extension("sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(lane.join("Cargo.toml"), "[package]\nname = \"planted\"\nversion = \"0.0.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::write(lane.join("src/lib.rs"), "").unwrap();
    let output = std::process::Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["capacity", "exec", "--"])
        .arg(&cargo)
        .args(arguments)
        .current_dir(&lane)
        .env("PATH", cargo.parent().unwrap())
        .env("HOME", &home)
        .env("CARGO_TARGET_DIR", scratch.path().join("target"))
        .env_remove("CARGO_HOME")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!marker.exists(), "a capacity run's cargo ran what a worker planted as {what}, so code a worker wrote ran outside its sandbox: {stderr}");
    assert!(!output.status.success(), "a run with {what} went on instead of refusing it: {stderr}");
    assert!(stderr.contains(&planted.display().to_string()), "the refusal of {what} does not name it: {stderr}");
}

fn gate_shim(path: &std::path::Path) -> bool {
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|file| std::io::Read::read_to_end(&mut std::io::Read::take(file, 512), &mut head)).is_ok() && head.windows(24).any(|window| window == b"north capacity gate shim")
}
