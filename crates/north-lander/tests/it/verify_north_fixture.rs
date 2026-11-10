use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use north_base::foundation::shim::write_executable;
use north_lander::land::queue;

use super::serve::{World, git};

const RECORDING_NORTH: &str = "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$XDG_STATE_HOME\" \"$NORTH_TEST_ROOT\" \"$*\" >> \"$0.calls\"\n";
const REMOTE_SSH: &str = "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o|-p) shift 2 ;; -*) shift ;; *) break ;; esac; done\nprintf '%s\\n' \"$1\" >> \"$FAKE_REMOTE_SSH_LOG\"\nshift\n[ \"$1\" = -- ] && shift\nexport XDG_STATE_HOME=\"$FAKE_REMOTE_STATE\"\nexec sh -c \"$*\"\n";
const REAL_QUEUE: &str = "north/land-queue/de2e0e8479546ba9";

struct Host {
    world: World,
    home: PathBuf,
    gex45: PathBuf,
    root: PathBuf,
    main: PathBuf,
    lane: PathBuf,
}

impl Host {
    fn new() -> Self {
        let world = World::new();
        let (home, gex45, root) = (world.path().join("home"), world.path().join("gex45-state"), world.path().join("verify-north-state"));
        fs::create_dir_all(home.join(".local/state").join(REAL_QUEUE)).unwrap();
        fs::create_dir_all(home.join(".local/state/north/fleet/proxy")).unwrap();
        fs::write(home.join(".local/state/north/fleet/proxy/inbox.jsonl"), "").unwrap();
        fs::create_dir_all(gex45.join(REAL_QUEUE)).unwrap();
        fs::create_dir_all(&root).unwrap();
        let orchestration = north_base::platform::orchestration::under(&home);
        fs::create_dir_all(orchestration.parent().unwrap()).unwrap();
        fs::write(&orchestration, "[hosts.gex45]\nssh = \"gex45\"\n").unwrap();
        write_executable(&world.path().join("bin/north"), RECORDING_NORTH);
        write_executable(&world.path().join("bin/ssh"), REMOTE_SSH);

        let scratch = world.path().join("verify-north-scratch");
        let (origin, main, lane) = (scratch.join("origin.git"), scratch.join("project/main"), scratch.join("project/worktrees/worker"));
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        fs::write(main.join("README.md"), "The scratch repository verify-north lands one worker's change into.\n").unwrap();
        let lander_host = north_base::platform::land_queue::local_host().split('.').next().unwrap().to_owned();
        fs::write(main.join(".safe-push"), format!("[landing]\n\tqueue = lander\n\tlander-host = {lander_host}\n\tremote-hosts =\n[land]\n\tcheck = ** :: true\n")).unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "Start the scratch repository"]);
        git(&scratch, &["clone", "-q", "--bare", main.to_str().unwrap(), origin.to_str().unwrap()]);
        git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&main, &["fetch", "-q", "origin"]);
        git(&main, &["worktree", "add", "-q", "-b", "worker", lane.to_str().unwrap(), "origin/main"]);
        fs::write(lane.join("worker.txt"), "the worker's change\n").unwrap();
        git(&lane, &["add", "worker.txt"]);
        git(&lane, &["commit", "-q", "-m", "Add the worker's change"]);
        Self { world, home, gex45, root, main, lane }
    }

    fn north_land(&self, process: &str, arguments: &[&str]) -> Output {
        let path = format!("{}:{}", self.world.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", process, "--nocapture", "--test-threads", "1"])
            .env("NORTH_LAND_IT_ARGUMENTS", arguments.join("\n"))
            .env("XDG_STATE_HOME", &self.root)
            .env("NORTH_TEST_ROOT", &self.root)
            .env("THREADS_DB", self.root.join("threads.db"))
            .env("HOME", &self.home)
            .env("PATH", path)
            .env("NORTH_FLEET_SESSION", "vn-1794159129")
            .env("FAKE_REMOTE_STATE", &self.gex45)
            .env("FAKE_REMOTE_SSH_LOG", self.world.path().join("ssh.calls"))
            .env("GIT_CONFIG_GLOBAL", self.world.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("AGENTS_ORCHESTRATION")
            .env_remove("AGENTS_OPERATOR_DIR")
            .env_remove("NORTH_LAND_NORTH")
            .env_remove("NORTH_DECISION_LOG")
            .env_remove("NORTH_RUNTIME_LINK")
            .env_remove("NORTH_PUSH_LOG")
            .env_remove("NORTH_LAND_SERVING")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path.clone());
                found.insert(path, Vec::new());
            } else {
                found.insert(path.clone(), fs::read(&path).unwrap_or_default());
            }
        }
    }
    found
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn the_verify_north_fixture_confines_every_path_to_its_own_root() {
    let host = Host::new();
    let (home, gex45) = (tree(&host.home), tree(&host.gex45));

    let landed = || git(&host.main, &["ls-remote", "origin", "refs/heads/main"]);
    let base = landed();
    let queued = host.north_land("serve::enqueue_process", &["--repo", host.lane.to_str().unwrap()]);
    let queued = text(&queued);
    assert!(queued.contains("Done: queued"), "safe-push did not queue the worker's lane:\n{queued}");
    let origin = git(&host.main, &["remote", "get-url", "--push", "origin"]);
    let fixture_queue = queue::directory(&host.root, &origin);
    assert!(queue::pending(&fixture_queue).is_ok_and(|tickets| tickets.len() == 1), "the ticket is not in the fixture's queue {}:\n{queued}", fixture_queue.display());

    let (checker, token) = (host.world.path().join("bin/checker"), host.world.path().join("bin/token"));
    let served = host.north_land("serve::lander_process", &["serve", "--once", "--repo", host.main.to_str().unwrap(), "--checker", checker.to_str().unwrap(), "--app-token", token.to_str().unwrap()]);
    let served = text(&served);
    assert_ne!(landed(), base, "the scratch lander never landed the worker's commit, so its paths went unexercised:\n{served}");
    git(&host.main, &["fetch", "-q", "origin"]);
    assert_eq!(git(&host.main, &["show", "origin/main:worker.txt"]), "the worker's change", "{served}");

    let root = host.root.to_string_lossy();
    let calls = fs::read_to_string(host.world.path().join("bin/north.calls")).unwrap_or_default();
    let escaped: Vec<&str> = calls.lines().filter(|call| !call.starts_with(&format!("{root}|{root}|"))).collect();
    assert!(escaped.is_empty(), "a notification or fleet call ran outside the fixture's XDG_STATE_HOME and NORTH_TEST_ROOT {root}: {escaped:#?}\n{served}");
    assert!(!host.world.path().join("ssh.calls").exists(), "the scratch lander reached the fleet host gex45:\n{served}");
    assert_eq!(tree(&host.gex45), gex45, "gex45's state changed:\n{served}");
    assert_eq!(tree(&host.home), home, "the real home's fleet, land queue or state changed:\n{queued}\n{served}");
}
