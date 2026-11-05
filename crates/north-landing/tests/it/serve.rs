use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use north_landing::land::queue::{self, Ticket};

const INCIDENT: &str = "#!/bin/sh\n[ \"$1\" = 0000000000000000000000000000000000000000 ] && exit 0\necho \"fatal: 'land-candidate' is already used by worktree at $PWD/../worktrees/worktrees/lander\" >&2\nexit 1\n";
const ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";
const GREEN: &str = "#!/bin/sh\n[ -z \"$HOLD\" ] || { : > \"$HOLD.started\"; while [ ! -e \"$HOLD.release\" ]; do sleep 0.05; done; }\necho 'safe-push: verdict green'\n";

#[test]
fn lander_process() {
    let Ok(arguments) = std::env::var(ARGUMENTS) else { return };
    let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
    std::process::exit(i32::from(north_landing::land::run(&arguments)));
}

fn git(dir: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(arguments).env("GIT_CONFIG_GLOBAL", dir.ancestors().find(|path| path.join("gitconfig").is_file()).unwrap().join("gitconfig")).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(output.status.success(), "git {arguments:?} in {}: {}", dir.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct World {
    root: tempfile::TempDir,
}

struct Repository {
    origin: PathBuf,
    checkout: PathBuf,
    queue: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("gitconfig"), "[user]\n\tname = t\n\temail = t@example.com\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n").unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("checker"), GREEN).unwrap();
        std::fs::write(bin.join("north"), "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n").unwrap();
        for program in ["checker", "north"] {
            Command::new("chmod").arg("+x").arg(bin.join(program)).status().unwrap();
        }
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn repository(&self, name: &str) -> Repository {
        let project = self.path().join(name);
        let origin = project.join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        let checkout = project.join("main");
        git(&project, &["clone", "-q", origin.to_str().unwrap(), "main"]);
        std::fs::write(checkout.join("README"), "start\n").unwrap();
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", "start"]);
        git(&checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        let queue = queue::directory(&self.path().join("state"), &git(&checkout, &["remote", "get-url", "--push", "origin"]));
        Repository { origin, checkout, queue }
    }

    fn lander(&self, repository: &Repository, hold: Option<&Path>) -> Child {
        let arguments = ["serve", "--once", "--repo", repository.checkout.to_str().unwrap(), "--depth", "1", "--checker", self.path().join("bin/checker").to_str().unwrap()].join("\n");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "serve::lander_process", "--nocapture", "--test-threads", "1"])
            .env(ARGUMENTS, arguments)
            .env("XDG_STATE_HOME", self.path().join("state"))
            .env("HOME", self.path())
            .env("NORTH_LAND_NORTH", self.path().join("bin/north"))
            .env("NORTH_RUNTIME_LINK", self.path().join("current"))
            .env("GIT_CONFIG_GLOBAL", self.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("HOLD")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(hold) = hold {
            command.env("HOLD", hold);
        }
        command.spawn().unwrap()
    }
}

impl Repository {
    fn lane(&self, branch: &str) -> String {
        let lane = self.checkout.parent().unwrap().join("worktrees").join(branch);
        git(&self.checkout, &["worktree", "add", "-q", "-b", branch, lane.to_str().unwrap(), "origin/main"]);
        std::fs::write(lane.join(format!("{branch}.txt")), "lane\n").unwrap();
        git(&lane, &["add", "-A"]);
        git(&lane, &["commit", "-q", "-m", &format!("{branch} (north#360)")]);
        let ticket = Ticket {
            origin: git(&lane, &["remote", "get-url", "--push", "origin"]),
            root: lane.clone(),
            host: std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned(),
            branch: branch.to_owned(),
            head: git(&lane, &["rev-parse", "HEAD"]),
            session: None,
            queued_at: "2026-11-05T00:00:00Z".to_owned(),
            first_commit_secs: None,
            several_commits: None,
            red_fix: None,
            urgent: None,
            decided_on: None,
            verdict: None,
            keep_lane: false,
        };
        let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros();
        queue::write(&self.queue.join(queue::file_name(micros, branch)), &ticket).unwrap();
        ticket.head
    }

    fn main(&self) -> String {
        git(&self.origin, &["rev-parse", "refs/heads/main"])
    }

    fn lands(&self, head: &str) -> bool {
        let lane = git(&self.origin, &["log", "-1", "--format=%s", "refs/heads/main"]);
        let subject = git(&self.checkout, &["log", "-1", "--format=%s", head]);
        lane == subject
    }
}

fn report(output: &Output) -> String {
    format!("exit {:?}\nstdout:\n{}\nstderr:\n{}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() {
        assert!(Instant::now() < deadline, "{} never appeared", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_repository_that_errors_every_round_does_not_stop_another_repository_landing_in_that_round() {
    let world = World::new();
    let north = world.repository("north");
    let wisp = world.repository("wisp");
    let landing = north.lane("north-lane");
    wisp.lane("wisp-lane");
    let hooks = wisp.checkout.parent().unwrap().join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("post-checkout"), INCIDENT).unwrap();
    Command::new("chmod").arg("+x").arg(hooks.join("post-checkout")).status().unwrap();
    git(&wisp.checkout, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let wisp_before = wisp.main();
    let wisp_lander = world.lander(&wisp, None);
    let north_lander = world.lander(&north, None);
    let wisp_round = wisp_lander.wait_with_output().unwrap();
    let north_round = north_lander.wait_with_output().unwrap();
    println!("north lander:\n{}\nwisp lander:\n{}", report(&north_round), report(&wisp_round));
    assert_eq!(wisp_round.status.code(), Some(1), "{}", report(&wisp_round));
    assert!(String::from_utf8_lossy(&wisp_round.stderr).contains("is already used by worktree"), "{}", report(&wisp_round));
    assert_eq!(wisp.main(), wisp_before);
    assert_eq!(north_round.status.code(), Some(0), "{}", report(&north_round));
    assert!(north.lands(&landing), "{}", report(&north_round));
    let calls = std::fs::read_to_string(world.path().join("bin/north.calls")).unwrap();
    let reaps: Vec<&str> = calls.lines().filter(|call| call.starts_with("capacity offload reap")).collect();
    let owner = |repository: &Repository| format!("capacity offload reap --owner north-land-{}", repository.queue.file_name().unwrap().to_str().unwrap());
    assert_eq!(reaps.len(), 2, "{calls}");
    assert!(reaps.contains(&owner(&north).as_str()) && reaps.contains(&owner(&wisp).as_str()), "{calls}");
    assert!(north.queue.join("done").read_dir().unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains("north-lane")), "{}", report(&north_round));
}

#[test]
fn a_second_lander_on_a_served_queue_exits_refusing_and_names_the_holder() {
    let world = World::new();
    let north = world.repository("north");
    let landing = north.lane("north-lane");
    let hold = world.path().join("hold");
    let first = world.lander(&north, Some(&hold));
    wait_for(&hold.with_extension("started"));
    let second = world.lander(&north, None).wait_with_output().unwrap();
    std::fs::write(hold.with_extension("release"), "").unwrap();
    let first_round = first.wait_with_output().unwrap();
    println!("second lander:\n{}", report(&second));
    assert_eq!(second.status.code(), Some(1), "{}", report(&second));
    let refusal = String::from_utf8_lossy(&second.stderr);
    assert!(refusal.contains("is already served by"), "{}", report(&second));
    assert!(refusal.contains(&format!("serving {}", north.checkout.display())), "{}", report(&second));
    assert!(refusal.contains("pid "), "{}", report(&second));
    assert_eq!(first_round.status.code(), Some(0), "{}", report(&first_round));
    assert!(north.lands(&landing), "{}", report(&first_round));
}
