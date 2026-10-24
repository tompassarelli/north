use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use rusqlite::Connection;

const MAGIC: &[u8] = b"NORTH-SNAPSHOT-PAGES 1\n";

const FAKE_SSH: &str = r#"#!/bin/sh
for a; do last=$a; done
[ "$last" = "north threads snapshot-pages" ] || exit 97
"$NORTH" threads snapshot-pages >"$SCRATCH/out" || exit $?
wc -c <"$SCRATCH/out" >"$SCRATCH/bytes"
half=$(( $(cat "$SCRATCH/bytes") / 2 ))
case "${MODE:-}" in
  truncate) head -c "$half" "$SCRATCH/out" ;;
  stall) echo $$ >"$SCRATCH/ssh.pid"; head -c "$half" "$SCRATCH/out"; touch "$SCRATCH/stalled"; exec sleep 30 ;;
  *) cat "$SCRATCH/out" ;;
esac
"#;

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
    snapshot: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("state")).unwrap();
        fs::write(root.join("bin/ssh"), FAKE_SSH).unwrap();
        fs::set_permissions(root.join("bin/ssh"), fs::Permissions::from_mode(0o755)).unwrap();
        let source = root.join("ledger.db");
        let snapshot = root.join("state/snapshot.db");
        Rig { _scratch: scratch, root, source, snapshot }
    }

    fn north(&self, arguments: &[&str], mode: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command.args(arguments)
            .env("HOME", &self.root)
            .env("NORTH", env!("CARGO_BIN_EXE_north"))
            .env("SCRATCH", &self.root)
            .env("THREADS_DB", &self.source)
            .env("MODE", mode)
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("NORTH_SSH_CONFIG")
            .env_remove("THREADS_NO_NATIVE")
            .env_remove("CLAUDE_CODE_REMOTE");
        command
    }

    fn pull(&self, mode: &str) -> Output {
        let snapshot = self.snapshot.display().to_string();
        self.north(&["threads", "snapshot-pull", "--force", "--snapshot", &snapshot, "ledger-host"], mode).output().unwrap()
    }

    fn spawn_pull(&self, mode: &str) -> Child {
        let snapshot = self.snapshot.display().to_string();
        self.north(&["threads", "snapshot-pull", "--force", "--snapshot", &snapshot, "ledger-host"], mode)
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
    }

    fn bytes(&self) -> usize {
        fs::read_to_string(self.root.join("bytes")).unwrap().trim().parse().unwrap()
    }

    fn leftovers(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.root.join("state")).unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != "snapshot.db" && name != "snapshot.db.tried").collect();
        names.sort();
        names
    }
}

fn ledger(path: &Path, rows: usize) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE runs(id INTEGER PRIMARY KEY, body TEXT NOT NULL);").unwrap();
    insert(&db, 1..rows + 1);
    db
}

fn insert(db: &Connection, ids: std::ops::Range<usize>) {
    for id in ids {
        db.execute("INSERT INTO runs VALUES (?1, ?2)", (id as i64, format!("run {id} {}", "x".repeat(300)))).unwrap();
    }
}

fn rows(path: &Path) -> Vec<(i64, String)> {
    all_rows(&Connection::open(format!("file:{}?immutable=1", path.display())).unwrap())
}

fn all_rows(db: &Connection) -> Vec<(i64, String)> {
    let mut statement = db.prepare("SELECT id, body FROM runs ORDER BY id").unwrap();
    statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

fn quick_check(path: &Path) -> String {
    let db = Connection::open(format!("file:{}?immutable=1", path.display())).unwrap();
    db.query_row("PRAGMA quick_check", [], |row| row.get(0)).unwrap()
}

fn be32(bytes: &[u8]) -> usize {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
}

#[test]
fn a_sync_after_a_few_inserts_moves_only_the_changed_pages_and_matches_the_source() {
    let rig = Rig::new();
    let db = ledger(&rig.source, 3000);
    let first = rig.pull("");
    assert!(first.status.success(), "{first:?}");
    let full = rig.bytes();
    let image = fs::read(&rig.snapshot).unwrap();
    assert!(full > image.len(), "{full} {}", image.len());
    assert_eq!(&image[18..20], &[1, 1]);
    insert(&db, 3001..3004);
    let second = rig.pull("");
    assert!(second.status.success(), "{second:?}");
    let changed = rig.bytes();
    assert!(changed < 8 * 4100 && changed * 20 < full, "changed {changed} of {full}");
    assert_eq!(quick_check(&rig.snapshot), "ok");
    assert_eq!(rows(&rig.snapshot), all_rows(&db));
    assert_eq!(rows(&rig.snapshot).len(), 3003);
    assert!(!Path::new(&format!("{}-wal", rig.snapshot.display())).exists());
    assert!(rig.pull("").status.success());
    assert_eq!(rig.bytes(), MAGIC.len() + 8 + 4 + 32);
    assert!(rig.leftovers().is_empty(), "{:?}", rig.leftovers());
}

#[test]
fn b_a_pull_that_fails_or_is_killed_mid_transfer_leaves_no_temporary_file_and_the_old_snapshot_intact() {
    let rig = Rig::new();
    let db = ledger(&rig.source, 3000);
    assert!(rig.pull("").status.success());
    let before = fs::read(&rig.snapshot).unwrap();
    db.execute("DELETE FROM runs WHERE id % 2 = 0", []).unwrap();
    insert(&db, 5000..5500);
    let failed = rig.pull("truncate");
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    assert_eq!(fs::read(&rig.snapshot).unwrap(), before);
    assert!(rig.leftovers().is_empty(), "{:?}", rig.leftovers());
    let mut child = rig.spawn_pull("stall");
    let begun = Instant::now();
    while !rig.root.join("stalled").exists() {
        assert!(begun.elapsed() < Duration::from_secs(20), "the stalled transport never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    child.kill().unwrap();
    child.wait().unwrap();
    let ssh = fs::read_to_string(rig.root.join("ssh.pid")).unwrap();
    let _ = Command::new("kill").arg(ssh.trim()).status();
    assert_eq!(fs::read(&rig.snapshot).unwrap(), before);
    assert!(rig.leftovers().is_empty(), "{:?}", rig.leftovers());
}

#[test]
fn c_freelist_leaf_pages_and_deleted_cells_arrive_zeroed() {
    let rig = Rig::new();
    let db = Connection::open(&rig.source).unwrap();
    db.execute_batch("PRAGMA secure_delete=OFF; CREATE TABLE runs(id INTEGER PRIMARY KEY, body TEXT NOT NULL); CREATE TABLE notes(v TEXT);").unwrap();
    for id in 1..=200 {
        let marker = if id <= 150 { "DELETED" } else { "KEPT" };
        db.execute("INSERT INTO runs VALUES (?1, ?2)", (id, format!("{marker}-{id} {}", "y".repeat(1000)))).unwrap();
    }
    db.execute_batch("INSERT INTO notes VALUES ('kept-one'), ('DELETED-note'), ('kept-two'); DELETE FROM notes WHERE v = 'DELETED-note'; DELETE FROM runs WHERE id <= 150;").unwrap();
    let contains = |bytes: &[u8], needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
    assert!(contains(&fs::read(&rig.source).unwrap(), b"DELETED-note"));
    let pulled = rig.pull("");
    assert!(pulled.status.success(), "{pulled:?}");
    let image = fs::read(&rig.snapshot).unwrap();
    let size = usize::from(u16::from_be_bytes([image[16], image[17]]));
    assert!(be32(&image[36..]) > 10, "freelist {}", be32(&image[36..]));
    let mut trunk = be32(&image[32..]);
    let mut leaves = 0;
    while trunk != 0 {
        let page = &image[(trunk - 1) * size..trunk * size];
        for index in 0..be32(&page[4..]) {
            let leaf = be32(&page[8 + 4 * index..]);
            assert!(image[(leaf - 1) * size..leaf * size].iter().all(|byte| *byte == 0), "leaf page {leaf} is not zero");
            leaves += 1;
        }
        trunk = be32(page);
    }
    assert!(leaves > 0);
    assert!(!contains(&image, b"DELETED-"), "a deleted row left the ledger host");
    assert!(contains(&image, b"KEPT-200"));
    assert_eq!(quick_check(&rig.snapshot), "ok");
}

#[test]
fn d_the_page_server_refuses_under_the_no_native_signals() {
    let rig = Rig::new();
    ledger(&rig.source, 10);
    let mut manifest = MAGIC.to_vec();
    manifest.extend([0; 8]);
    for (name, value) in [("THREADS_NO_NATIVE", "1"), ("CLAUDE_CODE_REMOTE", "true")] {
        let mut child = rig.north(&["threads", "snapshot-pages"], "").env(name, value)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        use std::io::Write;
        let _ = child.stdin.take().unwrap().write_all(&manifest);
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        let err = String::from_utf8(output.stderr).unwrap();
        assert!(err.starts_with("NorthThreads: the snapshot holds native items: ") && err.contains(name), "{err}");
    }
    let mut child = rig.north(&["threads", "snapshot-pages"], "")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(&manifest).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success() && output.stdout.starts_with(MAGIC), "{output:?}");
}
