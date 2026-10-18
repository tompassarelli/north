use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Lane {
    _fixture: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    origin: PathBuf,
}

impl Lane {
    fn new() -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().to_owned();
        let (repo, origin) = (root.join("repo"), root.join("origin.git"));
        fs::create_dir_all(repo.join(".github/workflows")).unwrap();
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Land branch scenario"]);
        git(&repo, &["config", "user.email", "land@example.invalid"]);
        git(
            &repo,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        fs::write(
            repo.join(".github/workflows/autoland.yml"),
            "on:\n  push:\n    branches: [\"claude/**\"]\n",
        )
        .unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "Autoland workflow"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["switch", "-qc", "lane"]);
        fs::write(repo.join("change"), "lane change\n").unwrap();
        git(&repo, &["add", "change"]);
        git(&repo, &["commit", "-qm", "lane change"]);
        let tools = root.join("tools");
        let runtime = root.join("home/.local/state/north/current");
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), runtime.join("north")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), tools.join("north")).unwrap();
        executable(&tools.join("gitleaks"), "#!/bin/sh\nexit 0\n");
        executable(
            &tools.join("gh"),
            "#!/bin/sh\ncase \"$1\" in repo) echo PUBLIC;; run) echo '[]';; api) cat \"$GH_AUTOLAND_STATUS\";; *) exit 1;; esac\n",
        );
        fs::write(
            root.join("status"),
            "pending\tAutoland workflow queued\thttps://example.test/run\n",
        )
        .unwrap();
        Lane {
            _fixture: fixture,
            root,
            repo,
            origin,
        }
    }

    fn safe_push(&self, arguments: &[&str]) -> Command {
        let mut paths = vec![self.root.join("tools")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let mut command = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/safe-push"));
        command
            .current_dir(&self.repo)
            .args(arguments)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("GH_AUTOLAND_STATUS", self.root.join("status"))
            .env("SAFE_PUSH_AUTOLAND_POLL_SECONDS", "1")
            .env_remove("SAFE_PUSH_TEST_REAL_NORTH");
        command
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.safe_push(arguments).output().unwrap()
    }

    fn land_branches(&self) -> Vec<String> {
        git(
            &self.origin,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads/claude/",
            ],
        )
        .lines()
        .map(str::to_owned)
        .collect()
    }

    fn queue(&self) -> String {
        let output = self
            .safe_push(&["--to", "main"])
            .env("SAFE_PUSH_AUTOLAND_WAIT_SECONDS", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let branches = self.land_branches();
        assert_eq!(branches.len(), 1, "{branches:?}");
        branches[0].clone()
    }
}

#[test]
fn interrupted_autoland_landing_keeps_its_queued_land_branch() {
    let lane = Lane::new();
    let mut child = lane
        .safe_push(&["--to", "main"])
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while lane.land_branches().is_empty() {
        assert!(
            Instant::now() < deadline,
            "safe-push never pushed its land branch"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "safe-push ended before it queued"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let group = format!("-{}", child.id());
    assert!(
        Command::new("kill")
            .args(["-TERM", "--", &group])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(child.wait().unwrap().code(), Some(130));
    assert_eq!(lane.land_branches().len(), 1);
}

#[test]
fn abandon_deletes_only_a_superseded_or_handed_back_land_branch() {
    let lane = Lane::new();
    let live = lane.queue();
    for arguments in [
        vec!["abandon"],
        vec!["abandon", live.as_str()],
        vec!["abandon", "main"],
        vec!["abandon", "lane"],
    ] {
        let output = lane.run(&arguments);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(lane.land_branches(), vec![live.clone()]);
    assert_eq!(
        git(&lane.origin, &["rev-parse", "main"]),
        git(&lane.repo, &["rev-parse", "main"])
    );
    fs::write(
        lane.root.join("status"),
        "error\tlocal refresh required: Classic effect record\thttps://example.test/run\n",
    )
    .unwrap();
    let output = lane.run(&["abandon", &live]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(lane.land_branches(), Vec::<String>::new());
    fs::write(
        lane.root.join("status"),
        "pending\tAutoland workflow queued\thttps://example.test/run\n",
    )
    .unwrap();
    let superseded = lane.queue();
    git(&lane.repo, &["switch", "-q", "main"]);
    fs::write(lane.repo.join("change"), "the fixed lane change\n").unwrap();
    git(&lane.repo, &["add", "change"]);
    git(&lane.repo, &["commit", "-qm", "lane change"]);
    git(&lane.repo, &["push", "-q", "origin", "main"]);
    git(&lane.repo, &["switch", "-q", "lane"]);
    let output = lane.run(&["abandon"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&format!("deleted {superseded} from origin"))
    );
    assert_eq!(lane.land_branches(), Vec::<String>::new());
}
