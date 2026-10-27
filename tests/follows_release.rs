use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;

const FLEET_NORTH: &str = r#"#!/usr/bin/env bash
if [ "$1 $2" = "fleet send" ]; then
  if [ -e "$scratch/bin/start-fails" ] && [[ "$*" == *"Staff this queued box now"* ]]; then exit 1; fi
  printf '%s\n' "$*" >>"$scratch/bin/fleet-sends"
  if [[ "$*" == *"Staff this queued box now"* ]]; then printf '%s %s\n' "$(date +%s.%N)" "$*" >>"$scratch/bin/started"; fi
else
  exec "$NORTH_LOCAL" "$@"
fi
"#;
const INSTALLED_NORTH: &str = "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$scratch/installed-calls\"\nexit 97\n";
const GH: &str = "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >>\"$scratch/gh-calls\"\ncat \"$scratch/gh-state\" 2>/dev/null && exit 0\nexit 1\n";
const NORTH: &str = env!("CARGO_BIN_EXE_north");

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    scratch: tempfile::TempDir,
    container: PathBuf,
}

impl Fixture {
    fn new(usage_gate: Option<String>) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        let container = root.join("code/north");
        for directory in ["installed", "bin", "home/.local/state/agents"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        executable(&root.join("installed/north"), INSTALLED_NORTH);
        executable(&root.join("bin/gitleaks"), "#!/usr/bin/env bash\nexit 0\n");
        executable(&root.join("bin/gh"), GH);
        executable(&root.join("bin/north"), FLEET_NORTH);
        std::os::unix::fs::symlink(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"), root.join("bin/threads")).unwrap();
        fs::write(root.join("orchestration.toml"), "").unwrap();
        let epoch = now();
        let gate = usage_gate.unwrap_or_else(|| {
            format!(r#"{{"claude":{{"epoch":{epoch},"stale_min":600,"refusal":null}},"codex":{{"epoch":{epoch},"stale_min":600,"refusal":null}}}}"#)
        });
        fs::write(root.join("home/.local/state/agents/usage-gate.json"), gate).unwrap();
        let fixture = Self { scratch, container };
        fixture.probe(0);
        fixture.seed_repository();
        fixture
    }

    fn root(&self) -> &Path {
        self.scratch.path()
    }

    fn seed_repository(&self) {
        let (remote, main) = (self.container.join("remote/north.git"), self.container.join("main"));
        fs::create_dir_all(&remote).unwrap();
        fs::create_dir_all(&main).unwrap();
        let identity = [("GIT_AUTHOR_NAME", "t"), ("GIT_AUTHOR_EMAIL", "t@example.invalid"), ("GIT_COMMITTER_NAME", "t"), ("GIT_COMMITTER_EMAIL", "t@example.invalid")];
        let run = |directory: &Path, arguments: &[&str]| {
            let output = Command::new("git").current_dir(directory).envs(identity).args(arguments).output().unwrap();
            assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        };
        run(&remote, &["init", "-q", "--bare", "-b", "main"]);
        run(&main, &["init", "-q", "-b", "main"]);
        run(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
        run(&main, &["remote", "add", "origin", remote.to_str().unwrap()]);
        run(&main, &["push", "-q", "-u", "origin", "main"]);
        let stamp = self.root().join("landed-at");
        let hook = format!(
            "#!/usr/bin/env bash\nwhile read -r _ _ ref; do [ \"$ref\" = refs/heads/main ] && [ ! -e \"{0}\" ] && date +%s.%N >\"{0}\"; done\nexit 0\n",
            stamp.display()
        );
        executable(&remote.join("hooks/post-receive"), &hook);
        let lane = self.container.join("worktrees/lane");
        run(&main, &["worktree", "add", "-q", lane.to_str().unwrap(), "-b", "lane"]);
        run(&lane, &["commit", "-q", "--allow-empty", "-m", "Land the predecessor box"]);
    }

    fn probe(&self, committed_cpus: u32) {
        fs::write(
            self.root().join("probe.json"),
            format!(r#"{{"committedBatchCpus":{committed_cpus},"aggregateCpuLimit":8,"protectedCpuSomeAvg10":0}}"#),
        )
        .unwrap();
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let root = self.root();
        let path = format!("{}:{}:{}", root.join("bin").display(), root.join("installed").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(program);
        command
            .env("PATH", path)
            .env("scratch", root)
            .env("NORTH_LOCAL", NORTH)
            .env("THREADS_DB", root.join("threads.db"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("SPAWN_CAPACITY_HOME", root.join("home"))
            .env("SPAWN_CAPACITY_STATUS", root.join("probe.json"))
            .env("AGENTS_ORCHESTRATION", root.join("orchestration.toml"))
            .env("FOLLOWS_RELEASE_ROOT", root.join("code"))
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .env_remove("FOLLOWS_RELEASE_NOW");
        command
    }

    fn north(&self, arguments: &[&str]) -> Output {
        self.command(NORTH).args(arguments).output().unwrap()
    }

    fn release(&self) -> String {
        let output = self.north(&["follows-release"]);
        String::from_utf8_lossy(&[output.stdout, output.stderr].concat()).into_owned()
    }

    fn release_at(&self, offset: u64) {
        self.command(NORTH).arg("follows-release").env("FOLLOWS_RELEASE_NOW", (now() + offset).to_string()).output().unwrap();
    }

    fn queue(&self, issue: u32, follows: &str, tier: &str) -> String {
        let brief = format!("Delegation: role=worker budget=0\nItem: north#{issue}\nFollows: {follows}\nCategory: feature\n");
        let mut child = self
            .command(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads"))
            .args(["queue", "follow", &format!("north#{issue}"), "--tier", tier, "--parent", "galileo-parent"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), brief.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.root().join(name)).unwrap_or_default()
    }

    fn started(&self) -> String {
        self.read("bin/started")
    }

    fn started_for(&self, issue: u32) -> usize {
        self.started().lines().filter(|line| line.contains(&format!("Item: north#{issue}"))).count()
    }

    fn listed(&self, issue: u32) -> usize {
        let output = self.command(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads")).args(["queue", "follow-list"]).output().unwrap();
        String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.contains(&format!("\"item\": \"north#{issue}\""))).count()
    }

    fn set_gh(&self, state: &str) {
        fs::write(self.root().join("gh-state"), format!("{state}\n")).unwrap();
    }

    fn reset_started(&self) {
        fs::write(self.root().join("bin/started"), "").unwrap();
    }

    fn start_fails(&self, failing: bool) {
        let marker = self.root().join("bin/start-fails");
        if failing {
            fs::write(marker, "").unwrap();
        } else {
            let _ = fs::remove_file(marker);
        }
    }

    fn head(&self) -> String {
        git(&self.container.join("main"), &["rev-parse", "HEAD"])
    }

    fn origin_main(&self) -> String {
        git(&self.container.join("main"), &["rev-parse", "origin/main"])
    }

    fn never_fell_back_to_the_installed_runtime(&self) {
        assert!(!self.root().join("installed-calls").exists(), "a north call fell back to the installed runtime: {}", self.read("installed-calls"));
    }
}

#[test]
fn landing_starts_a_follows_box_on_its_tier_and_stamps_only_mains_first_update() {
    let fixture = Fixture::new(None);
    fixture.queue(7, "Land the predecessor box", "codex:medium");
    let lane = fixture.container.join("worktrees/lane");
    let lane = lane.canonicalize().unwrap();
    let output = fixture
        .command("sh")
        .args(["-c", "\"$0\" --to main && pwd -P"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push"))
        .current_dir(&lane)
        .env("SAFE_PUSH_TEST_REAL_NORTH", NORTH)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&[output.stdout.clone(), output.stderr.clone()].concat()).into_owned();
    let landed_at: f64 = fixture.read("landed-at").trim().parse().unwrap_or(0.0);
    let started = fixture.started();
    let started_at: f64 = started.split_whitespace().next().and_then(|at| at.parse().ok()).unwrap_or(f64::MAX);
    assert!(output.status.success(), "{out}");
    fixture.never_fell_back_to_the_installed_runtime();
    assert!(started_at - landed_at < 60.0, "{started_at} {landed_at}");
    assert!(started.contains("fleet send galileo-parent") && started.contains("fresh worker at codex:medium"), "{started}");
    assert!(out.contains("box 1 north#7: started") && Regex::new(r"safe-push: landed [0-9a-f]{40}").unwrap().is_match(&out), "{out}");
    assert!(out.contains(&format!("cd {}/main when finished in the lane", fixture.container.display())), "{out}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().last(), lane.to_str(), "{out}");

    let stamp = fixture.read("landed-at");
    git(&fixture.container.join("main"), &["push", "-q", "origin", "HEAD:refs/heads/after-main"]);
    assert!(!stamp.is_empty() && fixture.read("landed-at") == stamp, "the landing stamp moved");
}

#[test]
fn a_box_denied_by_the_spawn_gate_and_one_not_started_within_five_minutes_each_alert_the_parent_once() {
    let fixture = Fixture::new(None);
    git(&fixture.container.join("worktrees/lane"), &["push", "-q", "origin", "lane:main"]);
    fixture.probe(8);
    fixture.queue(8, "Land the predecessor box", "worker");
    fixture.north(&["follows-release"]);
    fixture.probe(0);
    fixture.start_fails(true);
    fixture.queue(9, &fixture.origin_main(), "codex:high");
    fixture.north(&["follows-release"]);
    fixture.release_at(301);
    fixture.release_at(601);
    let sends = fixture.read("bin/fleet-sends");
    let alerts = |needle: &str| sends.lines().filter(|line| line.starts_with("fleet send galileo-parent FOLLOWS ") && line.contains(needle)).count();
    assert_eq!(alerts("box 1 north#8 "), 1, "{sends}");
    assert!(sends.lines().any(|line| line.contains("box 1 north#8 ") && line.contains("denied by the spawn gate")), "{sends}");
    assert_eq!(alerts("box 2 north#9 "), 1, "{sends}");
    assert!(sends.lines().any(|line| line.contains("box 2 north#9 ") && line.contains("not started within 5 minutes")), "{sends}");
    assert_eq!(sends.lines().filter(|line| line.starts_with("fleet send galileo-parent FOLLOWS")).count(), 2, "{sends}");
    fixture.never_fell_back_to_the_installed_runtime();
}

#[test]
fn a_released_box_is_dropped_once_its_issue_closes_or_the_parent_marks_it_staffed() {
    let fixture = Fixture::new(None);
    let sha = fixture.head();

    fixture.reset_started();
    fixture.start_fails(true);
    fixture.queue(30, &sha, "claude:opus-medium");
    fixture.north(&["follows-release"]);
    fixture.start_fails(false);
    fixture.set_gh("CLOSED");
    let out = fixture.release();
    fixture.north(&["follows-release"]);
    assert!(fixture.started_for(30) == 0 && fixture.listed(30) == 0 && out.contains("box 1 north#30: its issue is closed"), "{out}");

    fs::remove_file(fixture.root().join("gh-state")).unwrap();
    fixture.start_fails(true);
    let id = fixture.queue(31, &sha, "claude:opus-medium");
    fixture.north(&["follows-release"]);
    fixture.start_fails(false);
    fixture.north(&["threads", "queue", "follow-started", &id]);
    fixture.north(&["follows-release"]);
    assert!(fixture.started_for(31) == 0 && fixture.listed(31) == 0, "a box staffed elsewhere was sent again: {} {}", fixture.started(), id);

    fixture.queue(32, &sha, "claude:opus-medium");
    fixture.north(&["follows-release"]);
    let staffing = fixture.started();
    assert!(Regex::new(r"north threads queue follow-started 3[^0-9]").unwrap().is_match(&staffing), "{staffing}");
    fixture.never_fell_back_to_the_installed_runtime();
}

#[test]
fn a_follows_line_is_released_by_its_issue_closing_its_sha_or_its_commit_subject() {
    let fixture = Fixture::new(Some(format!(
        r#"{{"claude":{{"epoch":{},"stale_min":600,"refusal":null}},"codex":{{"epoch":1,"stale_min":12168,"refusal":null}}}}"#,
        now()
    )));
    fixture.reset_started();
    fixture.queue(20, &fixture.head(), "claude:opus-medium");
    fixture.north(&["follows-release"]);
    assert!(fixture.started().contains("fresh worker at claude:opus-medium"), "a claude tier box must pass the Claude spawn gate while codex is stale");

    let main = fixture.container.join("main");
    fixture.queue(21, "north#50", "claude:opus-medium");
    git(&main, &["pull", "-q", "--ff-only", "origin", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "Unrelated change (#50)"]);
    git(&main, &["push", "-q", "origin", "main"]);
    fixture.reset_started();
    fixture.set_gh("OPEN");
    fixture.north(&["follows-release"]);
    let open_starts = fixture.started_for(21);
    fixture.set_gh("CLOSED");
    fixture.north(&["follows-release"]);
    assert!(open_starts == 0 && fixture.started_for(21) == 1, "an unrelated (#N) subject released an open issue's box");

    fixture.queue(22, "wisp#51", "claude:opus-medium");
    fixture.north(&["follows-release"]);
    let calls = fixture.read("gh-calls");
    assert!(Regex::new(r"(?m)^issue view 51 --repo [^ /]+/wisp ").unwrap().is_match(&calls), "{calls}");
    assert!(!Regex::new(r"(?m)^issue view 51 --repo [^ /]+/north ").unwrap().is_match(&calls), "{calls}");
    assert!(fixture.started().contains("Item: north#22"));

    fixture.queue(23, "north#240 (n240-ledger, worker-ledger recording fix)", "claude:opus-medium");
    fixture.reset_started();
    fixture.set_gh("OPEN");
    fixture.north(&["follows-release"]);
    let note_open = fixture.started_for(23);
    fixture.set_gh("CLOSED");
    fixture.north(&["follows-release"]);
    assert!(note_open == 0 && fixture.started_for(23) == 1, "an issue then a note must release only once the issue closes");
    assert!(Regex::new(r"(?m)^issue view 240 --repo [^ /]+/north ").unwrap().is_match(&fixture.read("gh-calls")));

    fixture.reset_started();
    fixture.queue(24, &format!("{} the predecessor commit", fixture.origin_main()), "claude:opus-medium");
    fixture.north(&["follows-release"]);
    assert_eq!(fixture.started_for(24), 1, "a SHA then a note is released once the commit is on origin/main");

    fixture.queue(26, "Unrelated change (#50)", "claude:opus-medium");
    fixture.reset_started();
    fixture.north(&["follows-release"]);
    assert_eq!(fixture.started_for(26), 1, "an exact commit subject on origin/main is released");
    fixture.never_fell_back_to_the_installed_runtime();
}
