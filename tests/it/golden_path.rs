use crate::fleet_launch;

use north_core::foundation::shim;
use north_landing::land::queue;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

const BRIEF: &str = "Item: north#1 golden path\nCategory: tooling\nSpec: exact\nScope: one-file\nSurface: rust\nVerify: local-test\nWrite the sample change and commit it.\n";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn eventually<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(found) = probe() {
            return found;
        }
        assert!(Instant::now() < deadline, "never saw {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn host_label() -> String {
    let hostname = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    hostname.trim().split('.').next().unwrap().to_owned()
}

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
    origin: PathBuf,
    main: PathBuf,
    lane: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().canonicalize().unwrap();
        fleet_launch::fixture(&root, "commit");
        let (origin, main, lane) = (root.join("origin.git"), root.join("proj/main"), root.join("proj/worktrees/worker"));
        let tools = root.join("bin");
        let runtime = root.join(".local/state/north/current");
        fs::create_dir_all(&runtime).unwrap();
        for link in [tools.join("north"), runtime.join("north")] {
            std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), link).unwrap();
        }
        shim::write_script(&tools.join("betterleaks"), "exit 0\n");
        shim::write_script(&tools.join("gh"), "case \"$1\" in repo) echo PUBLIC;; issue) echo '[]';; *) exit 1;; esac\n");
        git(&root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "Golden path"), ("user.email", "golden@example.invalid"), ("commit.gpgsign", "false")] {
            git(&main, &["config", key, value]);
        }
        git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
        fs::write(main.join("README.md"), "sample\n").unwrap();
        fs::write(main.join(".safe-push"), format!("[landing]\n\tqueue = lander\n\tlander-host = {}\n\tremote-hosts =\n[land]\n\tcheck = ** :: printf checked >> \"$GOLDEN_CHECKS\"\n", host_label())).unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-qm", "Start the sample repository"]);
        git(&main, &["push", "-q", "origin", "main"]);
        git(&main, &["fetch", "-q", "origin"]);
        git(&main, &["worktree", "add", "-q", "-b", "worker", lane.to_str().unwrap(), "origin/main"]);
        Rig { _scratch: scratch, root, origin, main, lane }
    }

    fn north(&self, args: &[&str]) -> Output {
        fleet_launch::environment(Command::new(env!("CARGO_BIN_EXE_north")), &self.root, "here").env("FAKE_CLAUDE_PROJECT", "-home-tom-code-proj-worktrees-worker").args(args).output().unwrap()
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>, directory: &Path) -> Command {
        let mut command = fleet_launch::environment(Command::new(program), &self.root, "here");
        command
            .current_dir(directory)
            .env("GOLDEN_CHECKS", self.root.join("checks"))
            .env("NORTH_LAND_NORTH", env!("CARGO_BIN_EXE_north"))
            .env_remove("NORTH_LAND_SERVING")
            .env_remove("SAFE_PUSH_LAND_CAPACITY")
            .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
            .env_remove("SAFE_PUSH_LAND_RUNNER");
        command
    }

    fn safe_push(&self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push")
    }

    fn launch_worker(&self) -> Value {
        let lane = self.lane.display().to_string();
        let launched = self.north(&["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "worker", "--parent", "reader", "--local", "hermetic golden path", "--cwd", &lane, BRIEF]);
        assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
        eventually("the worker's Done report", || {
            let show = self.north(&["fleet", "show", "worker", "--json"]);
            let shown: Value = serde_json::from_slice(&show.stdout).ok()?;
            (shown["status"] != "working").then_some(shown)
        })
    }
}

fn lingering(root: &Path) -> Vec<String> {
    let mine = std::process::id().to_string();
    let marker = root.to_string_lossy().into_owned();
    fs::read_dir("/proc")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()) && entry.file_name().to_string_lossy() != mine)
        .filter(|entry| {
            let cmdline = fs::read(entry.path().join("cmdline")).unwrap_or_default();
            let cwd = fs::read_link(entry.path().join("cwd")).unwrap_or_default();
            String::from_utf8_lossy(&cmdline).contains(&marker) || cwd.starts_with(root)
        })
        .map(|entry| String::from_utf8_lossy(&fs::read(entry.path().join("cmdline")).unwrap_or_default()).replace('\0', " "))
        .collect()
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn rows(database: &Path, query: &str, columns: usize) -> Vec<Vec<String>> {
    eventually("the ledger rows", || {
        let connection = rusqlite::Connection::open(database).ok()?;
        let mut statement = connection.prepare(query).ok()?;
        let found: Vec<Vec<String>> = statement
            .query_map([], |row| (0..columns).map(|index| row.get::<_, rusqlite::types::Value>(index).map(|value| match value {
            rusqlite::types::Value::Text(text) => text,
            rusqlite::types::Value::Integer(number) => number.to_string(),
            rusqlite::types::Value::Null => String::new(),
            other => format!("{other:?}"),
        })).collect())
            .ok()?
            .collect::<Result<_, _>>()
            .ok()?;
        (!found.is_empty()).then_some(found)
    })
}

#[test]
fn a_brief_becomes_a_worker_commit_that_the_lander_lands_on_main_and_the_records_and_lane_show_it() {
    let started = Instant::now();
    let rig = Rig::new();
    let base = git(&rig.origin, &["rev-parse", "refs/heads/main"]);

    let launched = rig.launch_worker();
    let conversation = launched["conversation"].as_str().unwrap().to_owned();
    assert_eq!(launched["task_ref"], "north#1", "{launched}");
    let report = launched["entries"].as_array().unwrap().iter().rev().find(|entry| entry["kind"] == "assistant").unwrap();
    assert!(report["text"].as_str().unwrap().starts_with("Done:"), "{launched}");
    assert_eq!(git(&rig.lane, &["rev-list", "--count", &format!("{base}..HEAD")]), "1", "the worker committed once in its lane");
    assert_eq!(git(&rig.lane, &["show", "--format=", "--name-only", "HEAD"]), "worker.txt");
    let lane_head = git(&rig.lane, &["rev-parse", "HEAD"]);

    let enqueued = rig.command(rig.safe_push(), &rig.lane).args(["--to", "main"]).env("NORTH_FLEET_SESSION", "worker").output().unwrap();
    let said = text(&enqueued);
    assert!(enqueued.status.success(), "{said}");
    for line in ["language-gate: passed", "build-cache-gate: passed", &format!("Done: queued {} worker {lane_head}", rig.lane.display())] {
        assert!(said.contains(line), "enqueue never printed {line:?}: {said}");
    }
    assert_eq!(git(&rig.origin, &["rev-parse", "refs/heads/main"]), base, "enqueueing leaves main where it was");
    let tickets = queue::pending(&queue::directory(&rig.root.join(".local/state"), rig.origin.to_str().unwrap())).unwrap();
    assert_eq!(tickets.len(), 1, "{said}");
    assert_eq!((tickets[0].1.branch.as_str(), tickets[0].1.head.as_str(), tickets[0].1.session.as_deref()), ("worker", lane_head.as_str(), Some("worker")));
    assert!(!rig.root.join("checks").exists(), "no land check ran before the lander took the ticket");

    let served = rig.command(env!("CARGO_BIN_EXE_north"), &rig.main).args(["land", "serve", "--once", "--depth", "1", "--repo"]).arg(&rig.main).arg("--checker").arg(rig.safe_push()).output().unwrap();
    let said = text(&served);
    assert!(served.status.success(), "{said}");
    let landed = git(&rig.origin, &["rev-parse", "refs/heads/main"]);
    assert_ne!(landed, base, "{said}");
    assert!(said.contains(&format!("worker ({lane_head}): landed {landed}")), "{said}");
    assert_eq!(fs::read_to_string(rig.root.join("checks")).unwrap(), "checked", "the land check ran exactly once on the candidate");
    assert!(Command::new("git").arg("-C").arg(&rig.origin).args(["merge-base", "--is-ancestor", &landed, "refs/heads/main"]).status().unwrap().success());
    assert_eq!(git(&rig.origin, &["rev-parse", &format!("{landed}^")]), base, "the landing is the one lane commit on the old main");
    assert_eq!(git(&rig.origin, &["rev-parse", &format!("{landed}^{{tree}}")]), git(&rig.lane, &["rev-parse", "HEAD^{tree}"]), "main holds exactly the lane's tree");
    assert_eq!(git(&rig.origin, &["log", "-1", "--format=%s", &landed]), "Add the worker's change");
    assert_eq!(git(&rig.origin, &["show", "main:worker.txt"]), "the worker's change");
    assert!(queue::pending(&queue::directory(&rig.root.join(".local/state"), rig.origin.to_str().unwrap())).unwrap().is_empty());

    let database = rig.root.join("threads.db");
    let landings = rows(&database, "SELECT lane, verdict, landed_sha, batch_size, attempts FROM landings", 5);
    assert_eq!(landings, vec![vec!["worker".to_owned(), "landed".into(), landed.clone(), "1".into(), "1".into()]]);

    let notified = eventually("the landed message in the worker's session", || {
        let shown: Value = serde_json::from_slice(&rig.north(&["fleet", "show", "worker", "--json"]).stdout).ok()?;
        shown["entries"].as_array()?.iter().any(|entry| entry["text"] == format!("[from service:north-land] landed worker as {landed}")).then_some(shown)
    });
    assert_eq!(notified["conversation"], conversation.as_str());

    git(&rig.main, &["fetch", "-q", "origin"]);
    let profile = rig.root.join(".local/state/north/profiles/claude-a");
    let recorded = rig.command(env!("CARGO_BIN_EXE_north"), &rig.root).args(["worker-ledger", "--session", &conversation, "--since", "2026-11-01"]).env("CLAUDE_CONFIG_DIR", &profile).env("LEDGER_REPOS", &rig.main).output().unwrap();
    assert!(recorded.status.success(), "{}", text(&recorded));
    let runs = rows(&database, "SELECT agent, item, outcome, role, landed, landed_sha, category, spec, scope, verify FROM runs", 10);
    let expected: Vec<String> = [conversation.as_str(), "north#1", "done", "worker", "1", landed.as_str(), "tooling", "exact", "one-file", "local-test"].map(str::to_owned).to_vec();
    assert_eq!(runs, vec![expected], "{}", text(&recorded));

    let stopped = rig.north(&["fleet", "stop", "worker"]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    eventually("every process of the run to end", || lingering(&rig.root).is_empty().then_some(()));
    let retired = rig.command(env!("CARGO_BIN_EXE_north"), &rig.root).args(["lane-gc", "--root"]).arg(&rig.root).args(["--idle-hours", "0", "--state"]).arg(rig.root.join("gc.json")).output().unwrap();
    assert!(retired.status.success(), "{}", text(&retired));
    assert!(text(&retired).contains("retired 1 worktrees, 1 local branches"), "{}", text(&retired));
    assert!(!rig.lane.exists(), "the landed lane is gone");
    assert_eq!(git(&rig.main, &["branch", "--list", "worker"]), "");
    assert!(!git(&rig.main, &["worktree", "list", "--porcelain"]).contains(&rig.lane.display().to_string()));
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}
