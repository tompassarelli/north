use regex::Regex;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use north_core::foundation::shim;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GH: &str = r#"printf '%s\n' "$*" >>"$GH_TRACE"
issue() { printf '{"number":%s,"title":"T%s","blockedBy":{"nodes":[%s]}}' "$1" "$1" "${2:-}"; }
blocker() { printf '{"number":%s,"state":"%s","repository":{"nameWithOwner":"tompassarelli/makgora"}}' "$1" "$2"; }
case "$*" in
  *name=makgora*number=3*) printf '{"data":{"repository":{"issue":{"number":3,"title":"T3","state":"OPEN","blockedBy":{"nodes":[]}}}}}\n'; exit ;;
esac
case "$*" in
  *name=makgora*) nodes="$(issue 1 "$(blocker 2 OPEN)"),$(issue 2),$(issue 3),$(issue 4 "$(blocker 5 CLOSED)")" ;;
  *) nodes="" ;;
esac
printf '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[%s]}}}}\n' "$nodes"
"#;

const SSH: &str = r#"[ -e "$SSH_DOWN" ] && exit 255
if [ -f "$HOME/.config/threads/ssh-config" ]; then
  [ "${1:-}" = -F ] && [ "${2:-}" = "$HOME/.config/threads/ssh-config" ] || exit 255
fi
printf '%s\n' "${@: -1}" >>"$SSH_TRACE"
THREADS_DB=$REMOTE_DB PATH=$REMOTE_BIN:$PATH exec bash -c "${@: -1}"
"#;

fn bash(path: &Path, body: &str) {
    shim::write_executable(path, format!("#!/usr/bin/env bash\n{body}"));
}

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        bash(&root.join("bin/gh"), GH);
        fs::write(root.join("orchestration.toml"), "").unwrap();
        Rig { _scratch: scratch, root }
    }

    fn db(&self) -> PathBuf {
        self.root.join("threads.db")
    }

    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .arg(program)
            .args(args)
            .env("HOME", self.root.join("home"))
            .env("THREADS_DB", self.db())
            .env("GH_TRACE", self.root.join("gh.trace"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("NORTH_BIN", env!("CARGO_BIN_EXE_north"))
            .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
            .env("THREADS_ALIVE_OWNERS", r#"["alice","bob","fixture"]"#)
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env_remove("THREADS_NO_NATIVE")
            .env_remove("CLAUDE_CODE_REMOTE")
            .env_remove("THREADS_ACTOR")
            .env_remove("XDG_STATE_HOME");
        command
    }

    fn feed(mut command: Command, stdin: &str) -> (i32, String, String) {
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        (output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
    }

    fn threads(&self, args: &[&str]) -> (i32, String, String) {
        Self::feed(self.command("threads", args), "")
    }

    /// stdout then stderr, as `2>&1` collects a quick command's output.
    fn ingest(&self, rows: &[Value]) {
        let batch: String = rows.iter().map(|row| format!("{row}\n")).collect();
        let (code, _, err) = Self::feed(self.command("threads", &["ingest"]), &batch);
        assert_eq!(code, 0, "{err}");
    }

    fn sql(&self, query: &str) -> String {
        let db = Connection::open(self.db()).unwrap();
        db.query_row(query, [], |row| row.get::<_, rusqlite::types::Value>(0)).map(|value| match value {
            rusqlite::types::Value::Integer(n) => n.to_string(),
            rusqlite::types::Value::Text(text) => text,
            rusqlite::types::Value::Null => String::new(),
            other => format!("{other:?}"),
        }).unwrap()
    }
}

fn has(text: &str, pattern: &str) -> bool {
    Regex::new(&format!("(?m){pattern}")).unwrap().is_match(text)
}

#[test]
fn parallel_writers_keep_every_claim() {
    let rig = Rig::new();
    rig.ingest(&[]);
    let (writers, claims) = (8, 25);
    std::thread::scope(|scope| {
        for writer in 1..=writers {
            let rig = &rig;
            scope.spawn(move || {
                for claim in 0..claims {
                    let item = format!("wisp#{}", writer * 1000 + claim);
                    let (code, _, err) = rig.threads(&["claim", &item, "--by", &format!("w{writer}")]);
                    assert_eq!(code, 0, "{item}: {err}");
                }
            });
        }
    });
    assert_eq!(rig.sql("select count(*) from claims where item like 'wisp#%'"), (writers * claims).to_string());
}

struct Remote<'a> {
    rig: &'a Rig,
    home: PathBuf,
}

impl Remote<'_> {
    fn new(rig: &Rig) -> Remote<'_> {
        let home = rig.root.join("remote-home");
        fs::create_dir_all(home.join(".config/threads")).unwrap();
        fs::write(home.join(".config/threads/remote"), "nexus\n").unwrap();
        fs::create_dir_all(rig.root.join("remote")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), rig.root.join("remote/north")).unwrap();
        bash(&rig.root.join("bin/ssh"), SSH);
        Remote { rig, home }
    }

    fn run(&self, args: &[&str], stdin: &str) -> (i32, String) {
        let mut command = self.rig.command("threads", args);
        command
            .env_remove("THREADS_DB")
            .env("HOME", &self.home)
            .env("SSH_DOWN", self.rig.root.join("down"))
            .env("SSH_TRACE", self.rig.root.join("ssh.trace"))
            .env("REMOTE_DB", self.rig.root.join("remote.db"))
            .env("REMOTE_BIN", self.rig.root.join("remote"));
        let (code, out, err) = Rig::feed(command, stdin);
        (code, out + &err)
    }

    fn spooled(&self) -> usize {
        fs::read_dir(self.home.join(".local/state/threads/spool")).map_or(0, |entries| entries.filter_map(Result::ok).filter(|entry| !entry.file_name().to_string_lossy().starts_with('.')).count())
    }

    fn refreshes(&self) -> usize {
        fs::read_to_string(self.rig.root.join("ssh.trace")).unwrap_or_default().lines().filter(|line| *line == "north threads snapshot-pages").count()
    }
}

#[test]
fn the_ledger_host_over_ssh_spools_while_down_serves_a_stale_snapshot_and_flushes_in_order() {
    let rig = Rig::new();
    let remote = Remote::new(&rig);
    fs::write(rig.root.join("down"), "").unwrap();
    for id in ["s1", "s2"] {
        let row = json!({"id": id, "ended": "2026-10-09T01:00:00Z", "tier": "medium", "actual_min": 5, "outcome": "done", "brief": "Category: tooling"});
        assert_eq!(remote.run(&["ingest"], &format!("{row}\n")).0, 0);
    }
    let (status, _) = remote.run(&["claim", "wisp#9", "--by", "alice"], "");
    assert_eq!((status, remote.spooled()), (255, 2), "a claim fails while the host is down and both batches wait in the spool");
    fs::remove_file(rig.root.join("down")).unwrap();
    assert_eq!(remote.run(&["claim", "wisp#7", "--by", "bob"], "").0, 0);
    assert_eq!(remote.spooled(), 0);
    let snapshot = remote.home.join(".local/state/threads/snapshot.db");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !snapshot.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(snapshot.exists(), "a call that finds no snapshot pulls one in the background");
    fs::write(rig.root.join("ssh.trace"), "").unwrap();
    let goal = remote.run(&["calibrate", "--write", "--goal-row", r#"{"kind":"goal_eta","cell":"wisp","n":1,"value":5,"source":"fixture"}"#], "");
    assert_eq!((goal.0, remote.refreshes()), (0, 0), "a goal row writes without refreshing the snapshot: {}", goal.1);
    assert_eq!(remote.run(&["calibrate", "--write"], "").0, 0);
    assert_eq!(remote.refreshes(), 1, "other calibration writes refresh it");
    fs::write(rig.root.join("down"), "").unwrap();
    let (status, out) = remote.run(&["list"], "");
    assert!(status == 0 && has(&out, "^STALE: ledger snapshot from .*; nexus unreachable") && has(&out, r"^wisp#7 +bob"), "{out}");
    let order: String = Connection::open(rig.root.join("remote.db")).unwrap().query_row("select group_concat(agent) from (select agent from runs order by rowid)", [], |row| row.get(0)).unwrap();
    assert_eq!(order, "s1,s2");

    fs::remove_file(rig.root.join("down")).unwrap();
    fs::write(remote.home.join(".config/threads/ssh-config"), "Host nexus\n").unwrap();
    assert_eq!(remote.run(&["claim", "wisp#8", "--by", "configured-namespace-transport"], "").0, 0);
    assert_eq!(remote.run(&["calibrate", "--write"], "").0, 0);
    let held: i64 = Connection::open(rig.root.join("remote.db")).unwrap().query_row("select count(*) from claims where item='wisp#8' and owner='configured-namespace-transport'", [], |row| row.get(0)).unwrap();
    assert_eq!(held, 1, "the configured SSH namespace route keeps canonical writes");
}

