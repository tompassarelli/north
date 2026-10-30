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

impl Lane {
    fn supersede(&self) -> String {
        let branch = self.queue();
        git(&self.repo, &["switch", "-q", "main"]);
        fs::write(self.repo.join("change"), "the fixed lane change\n").unwrap();
        git(&self.repo, &["add", "change"]);
        git(&self.repo, &["commit", "-qm", "lane change"]);
        git(&self.repo, &["push", "-q", "origin", "main"]);
        git(&self.repo, &["switch", "-q", "lane"]);
        branch
    }

    fn land_abandon(&self, arguments: &[&str]) -> Output {
        let mut paths = vec![self.root.join("tools")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .arg("land-abandon")
            .args(arguments)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", self.root.join("home"))
            .env("GH_AUTOLAND_STATUS", self.root.join("status"))
            .output()
            .unwrap()
    }
}

#[test]
fn bare_land_abandon_deletes_a_superseded_land_branch_from_origin() {
    let lane = Lane::new();
    let superseded = lane.supersede();
    let output = lane.land_abandon(&[]);
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
    assert_eq!(
        git(
            &lane.repo,
            &["ls-remote", "origin", &format!("refs/heads/{superseded}")]
        ),
        ""
    );
}

#[test]
fn bare_land_abandon_fails_with_the_push_error_when_origin_refuses_the_delete() {
    let lane = Lane::new();
    let superseded = lane.supersede();
    git(&lane.origin, &["config", "receive.denyDeletes", "true"]);
    let output = lane.land_abandon(&[&superseded]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("denying ref deletion"), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("deleted"));
    assert_eq!(lane.land_branches(), vec![superseded]);
}


fn push_branch(lane: &Lane, branch: &str, file: &str, content: &str, subject: &str) -> String {
    git(&lane.repo, &["switch", "-q", "main"]);
    git(&lane.repo, &["switch", "-qc", "scratch-source"]);
    fs::write(lane.repo.join(file), content).unwrap();
    git(&lane.repo, &["add", file]);
    git(
        &lane.repo,
        &["commit", "-qm", subject, "--date=2001-01-01T00:00:00Z"],
    );
    git(
        &lane.repo,
        &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")],
    );
    let tip = git(&lane.repo, &["rev-parse", "HEAD"]);
    git(&lane.repo, &["switch", "-q", "main"]);
    git(&lane.repo, &["branch", "-qD", "scratch-source"]);
    tip
}

fn origin_has(lane: &Lane, branch: &str) -> bool {
    !git(
        &lane.origin,
        &["for-each-ref", &format!("refs/heads/{branch}")],
    )
    .is_empty()
}

#[test]
fn abandon_superseded_deletes_a_claude_or_farm_branch_only_when_main_holds_every_commit() {
    let lane = Lane::new();
    let by_subject = "claude/effect-upgrade-4.0.2-1";
    let by_patch = "farm/ll177-perf-split";
    let unlanded = "claude/voodoo51-base";
    push_branch(
        &lane,
        by_subject,
        "bump",
        "4.0.2\n",
        "Upgrade Effect to 4.0.2",
    );
    push_branch(&lane, by_patch, "perf", "split\n", "Split the perf run");
    push_branch(&lane, unlanded, "extra", "only here\n", "Measure voodoo51");
    push_branch(&lane, "probe/sc-324-dash", "dash", "x\n", "Probe dash");

    git(&lane.repo, &["switch", "-q", "main"]);
    fs::write(lane.repo.join("bump"), "4.0.3\n").unwrap();
    git(&lane.repo, &["add", "bump"]);
    git(&lane.repo, &["commit", "-qm", "Upgrade Effect to 4.0.2"]);
    fs::write(lane.repo.join("perf"), "split\n").unwrap();
    git(&lane.repo, &["add", "perf"]);
    git(
        &lane.repo,
        &["commit", "-qm", "Perf split landed under another name"],
    );
    git(&lane.repo, &["push", "-q", "origin", "main"]);

    for refused in [unlanded, "probe/sc-324-dash", "main"] {
        let output = lane.run(&["abandon", "--superseded", refused]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{refused}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(origin_has(&lane, unlanded));
    assert!(origin_has(&lane, "probe/sc-324-dash"));
    assert!(origin_has(&lane, "main"));
    let plain = lane.run(&["abandon", unlanded]);
    assert_eq!(plain.status.code(), Some(1));
    fs::write(
        lane.root.join("status"),
        "error\tAutoland workflow failed\thttps://example.test/run\n",
    )
    .unwrap();
    let handed_back = lane.run(&["abandon", "--superseded", unlanded]);
    assert_eq!(
        handed_back.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&handed_back.stderr)
    );
    assert!(origin_has(&lane, unlanded));
    fs::write(
        lane.root.join("status"),
        "pending\tAutoland workflow queued\thttps://example.test/run\n",
    )
    .unwrap();

    for branch in [by_subject, by_patch] {
        let output = lane.run(&["abandon", "--superseded", branch]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{branch}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!origin_has(&lane, branch));
    }
    assert!(origin_has(&lane, unlanded));
}

#[test]
fn abandon_deletes_a_land_branch_whose_amended_commit_landed_under_a_new_subject() {
    let lane = Lane::new();
    let first = lane.queue();
    let authored = git(&lane.repo, &["log", "-1", "--format=%aI", "lane"]);
    git(&lane.repo, &["switch", "-q", "main"]);
    fs::write(lane.repo.join("change"), "the amended lane change\n").unwrap();
    git(&lane.repo, &["add", "change"]);
    git(
        &lane.repo,
        &[
            "commit",
            "-qm",
            "lane change, reworded",
            &format!("--date={authored}"),
        ],
    );
    git(&lane.repo, &["push", "-q", "origin", "main"]);
    git(&lane.repo, &["switch", "-q", "lane"]);
    let output = lane.run(&["abandon", &first]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!origin_has(&lane, &first));
}

#[test]
fn an_amended_relanding_withdraws_the_lanes_older_land_branch() {
    let lane = Lane::new();
    let first = lane.queue();
    let neighbour = "claude/land-other-0123456789ab";
    push_branch(&lane, neighbour, "o", "o\n", "Other lane");
    git(&lane.repo, &["switch", "-q", "lane"]);
    git(
        &lane.repo,
        &["commit", "-q", "--amend", "-m", "lane change, reworded"],
    );
    let output = lane
        .safe_push(&["--to", "main"])
        .env("SAFE_PUSH_AUTOLAND_WAIT_SECONDS", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(2), "{stdout}");
    assert!(stdout.contains(&format!("withdrew {first}")), "{stdout}");
    assert!(!origin_has(&lane, &first));
    assert!(origin_has(&lane, neighbour));
    assert_eq!(lane.land_branches().len(), 2);
}
