use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use north_landing::land::queue::{self, Ticket};

const ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";
const GREEN: &str = "#!/bin/sh\n[ -z \"$HOLD\" ] || { : > \"$HOLD.started\"; while [ ! -e \"$HOLD.release\" ]; do sleep 0.05; done; }\necho 'safe-push: verdict green'\n";

#[test]
fn lander_process() {
    let Ok(arguments) = std::env::var(ARGUMENTS) else { return };
    let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
    std::process::exit(i32::from(north_landing::land::run(&arguments)));
}

pub(super) fn git(dir: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(arguments).env("GIT_CONFIG_GLOBAL", dir.ancestors().find(|path| path.join("gitconfig").is_file()).unwrap().join("gitconfig")).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(output.status.success(), "git {arguments:?} in {}: {}", dir.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(super) struct World {
    root: tempfile::TempDir,
}

pub(super) struct Repository {
    pub(super) origin: PathBuf,
    pub(super) checkout: PathBuf,
    pub(super) queue: PathBuf,
}

impl World {
    pub(super) fn new() -> Self {
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

    pub(super) fn repository(&self, name: &str) -> Repository {
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

    pub(super) fn lander(&self, repository: &Repository, hold: Option<&Path>) -> Child {
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
    pub(super) fn lane(&self, branch: &str) -> String {
        let lane = self.checkout.parent().unwrap().join("worktrees").join(branch);
        git(&self.checkout, &["worktree", "add", "-q", "-b", branch, lane.to_str().unwrap(), "origin/main"]);
        std::fs::write(lane.join(format!("{branch}.txt")), "lane\n").unwrap();
        git(&lane, &["add", "-A"]);
        git(&lane, &["commit", "-q", "-m", &format!("{branch} (north#360)")]);
        let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros();
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
            published: None,
            queued_micros: Some(micros),
        };
        queue::write(&self.queue.join(queue::file_name(micros, branch)), &ticket).unwrap();
        ticket.head
    }

}

pub(super) fn report(output: &Output) -> String {
    format!("exit {:?}\nstdout:\n{}\nstderr:\n{}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

