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
    refuses_planted("home/.cargo/bin/cargo-clippy", "home/code/proj/worktrees/lane", "~/.cargo/bin/cargo-clippy, a copy of the rustup and cargo beside it, under a system cargo first on PATH", |marker| format!("#!/bin/sh\n: > {}\n", marker.display()), &["clippy"]);
    runs_beside_rustup_proxies();
}

fn runs_beside_rustup_proxies() {
    let scratch = tempfile::tempdir().unwrap();
    let (home, marker) = (scratch.path().join("home"), scratch.path().join("ran"));
    let (bin, lane) = (home.join(".cargo/bin"), home.join("code/proj/worktrees/lane"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(lane.join(".git")).unwrap();
    north_core::foundation::shim::write_executable(&bin.join("rustup"), format!("#!/bin/sh\nexec {} \"$@\"\n", real_cargo().display()));
    north_core::foundation::shim::write_executable(&bin.join("cargo-clippy"), format!("#!/bin/sh\n: > {}\n", marker.display()));
    std::os::unix::fs::symlink("rustup", bin.join("cargo")).unwrap();
    let output = std::process::Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north")).args(["capacity", "exec", "--", "cargo", "clippy"]).current_dir(&lane).env("PATH", &bin).env("HOME", &home).env_remove("CARGO_HOME").output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success() && marker.exists(), "a host whose cargo is rustup's refused a gated cargo run over the proxies rustup keeps beside it: {stderr}");
}

fn refuses_planted(planted: &str, lane: &str, what: &str, content: impl Fn(&std::path::Path) -> String, arguments: &[&str]) {
    let mut cargo = real_cargo();
    let scratch = tempfile::tempdir().unwrap();
    let (home, lane, marker, planted) = (scratch.path().join("home"), scratch.path().join(lane), scratch.path().join("ran"), scratch.path().join(planted));
    std::fs::create_dir_all(home.join(".cargo")).unwrap();
    std::fs::create_dir_all(lane.join(".git")).unwrap();
    std::fs::create_dir_all(lane.join("src")).unwrap();
    std::fs::create_dir_all(planted.parent().unwrap()).unwrap();
    north_core::foundation::shim::write_executable(&marker.with_extension("sh"), format!("#!/bin/sh\n: > {}\nexec \"$@\"\n", marker.display()));
    north_core::foundation::shim::write_executable(&planted, content(&marker));
    let mut path = std::ffi::OsString::from(cargo.parent().unwrap());
    if planted.ends_with("cargo-clippy") {
        let (bin, system) = (planted.parent().unwrap().to_owned(), scratch.path().join("usr/bin"));
        for copy in ["rustup", "cargo"] {
            std::fs::copy(&planted, bin.join(copy)).unwrap();
        }
        std::fs::create_dir_all(&system).unwrap();
        north_core::foundation::shim::write_executable(&system.join("cargo"), format!("#!/bin/sh\nexec {} \"$@\"\n", cargo.display()));
        cargo = system.join("cargo");
        path = std::env::join_paths([system, bin]).unwrap();
    }
    std::fs::write(lane.join("Cargo.toml"), "[package]\nname = \"planted\"\nversion = \"0.0.0\"\nedition = \"2021\"\n").unwrap();
    std::fs::write(lane.join("src/lib.rs"), "").unwrap();
    let output = std::process::Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(["capacity", "exec", "--"])
        .arg(&cargo)
        .args(arguments)
        .current_dir(&lane)
        .env("PATH", &path)
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

fn real_cargo() -> std::path::PathBuf {
    std::env::var_os("CARGO").map(std::path::PathBuf::from).or_else(|| std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|directory| directory.join("cargo")).find(|path| path.is_file() && !gate_shim(path))).expect("a real cargo on PATH")
}

fn gate_shim(path: &std::path::Path) -> bool {
    let mut head = Vec::new();
    std::fs::File::open(path).and_then(|file| std::io::Read::read_to_end(&mut std::io::Read::take(file, 512), &mut head)).is_ok() && head.windows(24).any(|window| window == b"north capacity gate shim")
}
