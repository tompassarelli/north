use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CHILD: &str = "NORTH_706_REMOVE_COMPUTED";

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).status().unwrap();
    assert!(status.success(), "git {arguments:?}");
}

fn file(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "kept").unwrap();
}

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        let root = tempfile::Builder::new().prefix("north-706-home").tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let home = root.path();
        file(&home.join("Library/Application Support/App/cache.db"));
        file(&home.join("code/p/pins/abc/file"));
        file(&home.join("code/p/worktrees/other/work.rs"));
        file(&home.join("code/north-data/profiles/claude-a/projects/run/transcript.jsonl"));
        file(&home.join(".local/state/threads/threads.db"));
        file(&home.join(".local/state/convo/index.db"));
        file(&home.join("code/north-data/fleet/north-706/session.json"));
        file(&home.join(".local/state/btop.log"));
        std::os::unix::fs::symlink(home.join("code/north-data"), home.join(".local/state/north")).unwrap();
        let other = home.join("code/q/main");
        std::fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q", "-b", "main"]);
        let main = home.join("code/p/main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        file(&main.join("src.rs"));
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "one"]);
        git(&main, &["worktree", "add", "-q", "../worktrees/lane", "-b", "lane"]);
        file(&home.join("code/p/worktrees/lane/scratch/out.log"));
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn lane(&self) -> PathBuf {
        self.path().join("code/p/worktrees/lane")
    }

    fn removed(&self) -> Vec<PathBuf> {
        self.protected().into_iter().filter(|kept| !kept.exists()).collect()
    }

    fn protected(&self) -> Vec<PathBuf> {
        let home = self.path();
        ["Library/Application Support/App/cache.db", "code/p/pins/abc/file", "code/p/worktrees/other/work.rs", "code/p/main/src.rs", "code/q/main/.git/HEAD", ".local/state/north/profiles/claude-a/projects/run/transcript.jsonl", ".local/state/threads/threads.db", ".local/state/convo/index.db", ".local/state/north/fleet/north-706/session.json"].iter().map(|relative| home.join(relative)).collect()
    }

    fn run(&self, command: &str, sandboxed: bool) -> Output {
        self.run_in(&self.lane(), command, sandboxed)
    }

    fn run_in(&self, directory: &Path, command: &str, sandboxed: bool) -> Output {
        let mut outer = self.outer(directory);
        if sandboxed {
            outer.arg("--").arg(env!("CARGO_BIN_EXE_north")).args(["--worker-sandbox", command]);
        } else {
            outer.args(["--", "bash", "-c", command]);
        }
        outer.output().unwrap()
    }

    fn run_in_lane_only(&self, program: &[&str]) -> Output {
        self.run_in_lane_only_by(Path::new(env!("CARGO_BIN_EXE_north")), program)
    }

    fn run_in_lane_only_by(&self, north: &Path, program: &[&str]) -> Output {
        let mut outer = self.outer(&self.lane());
        outer.arg("--").arg(north).args(["--worker-sandbox", "--lane", "--"]).args(program);
        outer.output().unwrap()
    }

    fn outer(&self, directory: &Path) -> Command {
        let home = self.path();
        let mut outer = Command::new("bwrap");
        outer.args(["--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev", "--tmpfs", "/tmp", "--tmpfs", "/var/tmp", "--tmpfs", "/dev/shm", "--tmpfs"]);
        outer.arg(format!("/run/user/{}", rustix::process::getuid().as_raw()));
        outer.arg("--bind").arg(home).arg(home).arg("--chdir").arg(directory);
        outer.args(["--clearenv", "--setenv", "PATH"]).arg(std::env::var_os("PATH").unwrap()).arg("--setenv").arg("HOME").arg(home).args(["--setenv", "SHELL", "bash", "--setenv", "NORTH_FLEET_ROLE", "worker"]);
        outer.arg("--setenv").arg(CHILD).arg(home);
        outer
    }
}

#[test]
fn a_rust_remove_dir_all_on_a_computed_path_child() {
    let Some(home) = std::env::var_os(CHILD) else {
        return;
    };
    let wrong = Path::new(&home).join("Library/Application Support").parent().unwrap().to_path_buf();
    let removed = std::fs::remove_dir_all(&wrong);
    println!("remove_dir_all {}: {removed:?}", wrong.display());
}

#[test]
fn a_delete_whose_target_is_computed_at_run_time_cannot_remove_a_protected_directory_north_706() {
    let test = std::env::current_exe().unwrap();
    let replays = [
        r#"printf '%s\n' "$HOME/Library/Application Support/App/cache.db" | xargs -n1 dirname | xargs rm -rf"#.to_owned(),
        r#"rm -rf "$UNSET/""#.to_owned(),
        "find ~ -mindepth 1 -delete".to_owned(),
        format!("{} --exact worker_sandbox::a_rust_remove_dir_all_on_a_computed_path_child --nocapture", test.display()),
    ];
    for (index, replay) in replays.iter().enumerate() {
        let bare = Home::new();
        bare.run(replay, false);
        assert!(index == 1 || !bare.removed().is_empty(), "{replay} removes nothing even without the sandbox, so it proves nothing");
        let home = Home::new();
        let output = home.run(replay, true);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("not confined"), "{replay}: {stderr}");
        assert!(!stderr.contains("Read-only file system"), "{replay} reached past the throwaway home: {stderr}");
        assert!(home.removed().is_empty(), "{replay} removed {:?}: {stderr}", home.removed());
    }
    let home = Home::new();
    let output = home.run_in(&home.path().join("code/p/main"), &format!("cd {} && rm -rf scratch", home.lane().display()), true);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!home.lane().join("scratch").exists());
}

#[test]
fn a_worker_command_meets_a_null_device_at_every_runtime_socket_north_does_not_name_south_86() {
    let scratch = tempfile::Builder::new().prefix("s86").tempdir_in("/tmp").unwrap();
    let runtime = scratch.path().canonicalize().unwrap();
    std::fs::create_dir_all(runtime.join("manager")).unwrap();
    let _listeners: Vec<std::os::unix::net::UnixListener> = ["daemon.sock", "manager/private", "north-kept.sock", "wayland-1"].iter().map(|name| std::os::unix::net::UnixListener::bind(runtime.join(name)).unwrap()).collect();
    let command = r#"test -c "$XDG_RUNTIME_DIR/daemon.sock" && test -c "$XDG_RUNTIME_DIR/manager/private" && test -S "$XDG_RUNTIME_DIR/north-kept.sock" && test -S "$XDG_RUNTIME_DIR/wayland-1" && ! umount "$XDG_RUNTIME_DIR/daemon.sock""#;
    let output = Command::new(env!("CARGO_BIN_EXE_north")).args(["--worker-sandbox", command]).env_clear().env("PATH", std::env::var_os("PATH").unwrap()).env("HOME", &runtime).env("SHELL", "bash").env("NORTH_FLEET_ROLE", "worker").env("NORTH_WORKER_SANDBOX", "1").env("XDG_RUNTIME_DIR", &runtime).current_dir(&runtime).output().unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stderr).matches("north worker-sandbox:").count(), 0, "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state() {
    let home = Home::new();
    file(&home.path().join("code/q/main/README.md"));
    file(&home.path().join(".config/sops/age/keys.txt"));
    file(&home.path().join(".local/state/north/handoffs/brief.md"));
    file(&home.path().join("code/p/worktrees/lane/mine.txt"));
    let read = |relative: &str| home.run_in_lane_only(&["cat", home.path().join(relative).to_str().unwrap()]);

    for kept in ["code/p/worktrees/lane/mine.txt", "code/p/main/src.rs"] {
        let output = read(kept);
        assert!(output.status.success(), "{kept}: {}", String::from_utf8_lossy(&output.stderr));
    }
    for hidden in ["code/q/main/README.md", ".config/sops/age/keys.txt", ".local/state/north/handoffs/brief.md", "code/p/worktrees/other/work.rs"] {
        let output = read(hidden);
        assert!(!output.status.success() && String::from_utf8_lossy(&output.stderr).contains("Permission denied"), "{hidden} was read: {}", String::from_utf8_lossy(&output.stdout));
    }
}

#[test]
fn a_command_confined_to_its_lane_runs_the_toolchain_shims_and_the_north_runtime_that_run_a_lane_build_north_1020() {
    let home = Home::new();
    let script = |relative: &str, says: &str| {
        let path = home.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        north_core::foundation::shim::write_script(&path, &format!("echo {says}\n"));
        path
    };
    let shim = script(".local/share/north/bin/cargo", "shim");
    let on_path = script(".local/bin/north", "on-path");
    let runtime = script("code/north-data/runtimes/abc/north", "runtime");
    let opencode = script(".local/lib/opencode/current/bin/opencode", "opencode");
    let pi = script(".local/lib/pi/current/bin/pi", "pi");
    file(&home.path().join("code/north-data/accounts/key"));
    file(&home.path().join(".local/share/other/token"));
    file(&home.path().join(".local/lib/claude/versions/1/token"));
    let north = home.path().join(".local/state/north/runtimes/host/north");
    std::fs::create_dir_all(north.parent().unwrap()).unwrap();
    north_core::foundation::shim::copy_executable(env!("CARGO_BIN_EXE_north"), &north);

    for (program, says) in [(&shim, "shim"), (&on_path, "on-path"), (&runtime, "runtime"), (&opencode, "opencode"), (&pi, "pi")] {
        let output = home.run_in_lane_only_by(&north, &[program.to_str().unwrap()]);
        assert!(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == says, "{}: {}", program.display(), String::from_utf8_lossy(&output.stderr));
    }
    for hidden in ["code/north-data/accounts/key", ".local/share/other/token", ".local/lib/claude/versions/1/token"] {
        let output = home.run_in_lane_only_by(&north, &["cat", home.path().join(hidden).to_str().unwrap()]);
        assert!(!output.status.success() && String::from_utf8_lossy(&output.stderr).contains("Permission denied"), "{hidden} was read: {}", String::from_utf8_lossy(&output.stdout));
    }
}

#[test]
fn a_lane_created_after_the_session_started_runs_its_scripts_in_a_fresh_domain_that_hides_every_older_lane_north_1033() {
    let home = Home::new();
    let scratch = home.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let north = Path::new(env!("CARGO_BIN_EXE_north"));
    let session = |directory: &Path, fresh: bool, program: &[&str]| {
        let mut outer = home.outer(directory);
        outer.arg("--setenv").arg("NORTH_SANDBOX_SCRATCH").arg(&scratch);
        if fresh {
            outer.args(["--setenv", "NORTH_WORKER_LANE_ONLY", "1"]);
        }
        outer.arg("--").arg(north).args(["--worker-sandbox", "--lane", "--"]).args(program).output().unwrap()
    };
    let started = session(&home.lane(), false, &["true"]);
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));

    let fresh = home.path().join("code/p/worktrees/fresh");
    let tool = fresh.join("bin/tool");
    std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
    north_core::foundation::shim::write_script(&tool, "echo fresh-lane\n");
    file(&fresh.join("note.txt"));
    let path = |relative: &str| home.path().join(relative);

    let ran = session(&home.lane(), true, &[tool.to_str().unwrap()]);
    assert!(ran.status.success() && String::from_utf8_lossy(&ran.stdout).trim() == "fresh-lane", "{}", String::from_utf8_lossy(&ran.stderr));
    for kept in ["code/p/worktrees/fresh/note.txt", "code/p/main/src.rs"] {
        let read = session(&home.lane(), true, &["cat", path(kept).to_str().unwrap()]);
        assert!(read.status.success(), "{kept}: {}", String::from_utf8_lossy(&read.stderr));
    }
    for hidden in ["code/p/worktrees/other/work.rs", "code/q/main/.git/HEAD"] {
        let read = session(&home.lane(), true, &["cat", path(hidden).to_str().unwrap()]);
        assert!(!read.status.success() && String::from_utf8_lossy(&read.stderr).contains("Permission denied"), "{hidden} was read: {}", String::from_utf8_lossy(&read.stdout));
    }
    let elsewhere = session(&path("code/q/main"), true, &["cat", path("code/q/main/.git/HEAD").to_str().unwrap()]);
    assert!(!elsewhere.status.success() && String::from_utf8_lossy(&elsewhere.stderr).contains("outside the lane of this session"), "{}", String::from_utf8_lossy(&elsewhere.stdout));
    let older = session(&path("code/p/worktrees/other"), true, &["cat", path("code/p/worktrees/other/work.rs").to_str().unwrap()]);
    assert!(!older.status.success() && String::from_utf8_lossy(&older.stderr).contains("outside the lane of this session"), "{}", String::from_utf8_lossy(&older.stdout));
}

#[test]
fn a_lane_only_shell_ends_with_done_instead_of_running_safe_push_north_1041() {
    let home = Home::new();
    let lane = home.lane().canonicalize().unwrap();
    let head = Command::new("git").arg("-C").arg(&lane).args(["rev-parse", "HEAD"]).output().unwrap();
    assert!(head.status.success(), "{}", String::from_utf8_lossy(&head.stderr));
    let sha = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["safe-push", "--to", "main"])
        .current_dir(&lane)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", home.path())
        .env("NORTH_WORKER_SANDBOXED", "1")
        .env("NORTH_WORKER_LANE_ONLY", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains(&format!("lane-only session: end with Done: {} lane {sha}; the review-and-land loop lands it", lane.display())), "{stderr}");
    assert!(!stderr.contains("north worker-sandbox broker"), "the refusal names the hand-off, not the broker's opaque refusal: {stderr}");
}
