use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use rusqlite::Connection;

const FAKE_SSH: &str = r#"#!/bin/sh
for a; do last=$a; done
[ "$last" = "north threads snapshot-pages" ] || exit 97
echo refresh >>"$SCRATCH/trace"
if [ -n "$BAD_STREAM" ]; then printf 'SQLite format 3\0'; head -c 600 /dev/zero | tr '\0' x; exit 0; fi
THREADS_DB="$CANONICAL" exec "$NORTH" threads snapshot-pages
"#;

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        for directory in ["bin", ".config/threads", ".local/state/threads", "state", "op"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        fs::write(root.join("bin/ssh"), FAKE_SSH).unwrap();
        fs::set_permissions(root.join("bin/ssh"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(root.join(".config/threads/remote"), "far\n").unwrap();
        fs::write(root.join("orchestration.toml"), "").unwrap();
        let canonical = Connection::open(root.join("canonical.db")).unwrap();
        canonical
            .execute_batch(
                "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT; \
                 INSERT INTO usage VALUES ('2026-10-13T00:00:00Z', 'gemini', 'test', 'window', 'week', 12.0, 10080, NULL, NULL, 'recorded fixture');",
            )
            .unwrap();
        Self { _scratch: scratch, root }
    }

    fn snapshot(&self) -> PathBuf {
        self.root.join(".local/state/threads/snapshot.db")
    }

    fn corrupt(&self) {
        fs::write(self.snapshot(), [b"SQLite format 3\0".as_slice(), &[b'x'; 600]].concat()).unwrap();
    }

    fn trace(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("trace")).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    fn usage(&self, bad_stream: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["agents", "usage", "--json"])
            .env("HOME", &self.root)
            .env("NORTH", env!("CARGO_BIN_EXE_north"))
            .env("SCRATCH", &self.root)
            .env("CANONICAL", self.root.join("canonical.db"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("AGENTS_OPERATOR_DIR", self.root.join("op"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env_remove("THREADS_DB")
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("NORTH_SSH_CONFIG");
        if bad_stream {
            command.env("BAD_STREAM", "1");
        }
        command.output().unwrap()
    }
}

#[test]
fn agents_usage_repairs_a_corrupt_snapshot_once_over_ssh() {
    let rig = Rig::new();
    rig.corrupt();
    let output = rig.usage(false);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["used_pct"], 12.0);
    assert_eq!(rig.trace(), ["refresh"]);
    let verdict: String = Connection::open(rig.snapshot()).unwrap().query_row("PRAGMA quick_check", [], |row| row.get(0)).unwrap();
    assert_eq!(verdict, "ok");
}

#[test]
fn agents_usage_reports_persistent_corruption_after_one_retry_and_leaves_the_snapshot_alone() {
    let rig = Rig::new();
    rig.corrupt();
    let before = fs::read(rig.snapshot()).unwrap();
    let output = rig.usage(true);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("snapshot still unreadable after one refresh") && !stderr.contains("panicked"), "{stderr}");
    assert_eq!(rig.trace(), ["refresh"]);
    assert_eq!(fs::read(rig.snapshot()).unwrap(), before);
}
