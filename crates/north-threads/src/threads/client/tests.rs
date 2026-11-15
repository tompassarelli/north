use super::*;
use serde_json::Value;
use std::os::unix::fs::MetadataExt;
use std::collections::BTreeSet;

/// The live owners a test writes to a JSON file; it opens no gate.
struct FleetFile(PathBuf);

impl outside::FleetView for FleetFile {
    fn live_owners(&self) -> Option<BTreeSet<String>> {
        serde_json::from_slice::<Vec<String>>(&fs::read(&self.0).ok()?).ok().map(|owners| owners.into_iter().collect())
    }

    fn gated_issues(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}

struct Rig {
    _directory: tempfile::TempDir,
    root: PathBuf,
    client: Client,
}

const FAKE_SSH: &str = r#"for a; do last=$a; done
printf '%s\n' "$*" >> "$ROOT/ssh.log"
[ -f "$ROOT/stall" ] && { : > "$ROOT/stalled"; exec sleep 30; }
[ -f "$ROOT/refusal" ] && case "$last" in *"threads $(awk '{print $6}' "$ROOT/refusal")"*) cat "$ROOT/refusal" >&2; exit 126 ;; esac
case "$last" in
  *ingest) cat >> "$ROOT/ingest.log"; [ -f "$ROOT/hold" ] && while [ ! -f "$ROOT/go" ]; do sleep 0.1; done; [ -f "$ROOT/ingest-code" ] && exit "$(cat "$ROOT/ingest-code")" ;;
  "north threads snapshot-pages") cat > "$ROOT/manifest"; cat "$ROOT/pages" ;;
  "north fleet row put") { cat; echo; } >> "$ROOT/rows.log" ;;
esac
exit "$(cat "$ROOT/code")"
"#;

fn rig(env: &[(&str, &str)]) -> Rig {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    fs::create_dir_all(root.join(".config/threads")).unwrap();
    fs::create_dir_all(root.join(".local/state/threads")).unwrap();
    fs::write(root.join(".config/threads/remote"), "ledger-host\n").unwrap();
    fs::write(root.join("code"), "0").unwrap();
    fs::write(root.join("fleet.json"), "[]\n").unwrap();
    let ledger = root.join("ledger.db");
    rusqlite::Connection::open(&ledger).unwrap().execute_batch("CREATE TABLE t(v); INSERT INTO t VALUES (1);").unwrap();
    let mut pages = Vec::new();
    snapshot::serve(&ledger, false, &mut &snapshot::manifest_bytes(0, &[], false)[..], &mut pages).unwrap();
    fs::write(root.join("pages"), pages).unwrap();
    let ssh = format!("ROOT='{}'\n{FAKE_SSH}", root.display());
    let mut variables: BTreeMap<String, String> = env.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
    variables.insert("HOME".into(), root.display().to_string());
    let fleet_file = root.join("fleet.json");
    let fleet: Arc<dyn outside::FleetView> = Arc::new(FleetFile(fleet_file));
    let mut client = Client::new(
        &root,
        variables,
        vec!["/bin/sh".into(), "-c".into(), ssh.into(), "ssh".into()],
        fleet,
    );
    client.background = None;
    Rig { _directory: directory, root, client }
}

fn call(rig: &Rig, argv: &[&str], stdin: &[u8]) -> (i32, String) {
    let argv: Vec<String> = argv.iter().map(|arg| arg.to_string()).collect();
    let mut err = Vec::new();
    let code = rig.client.main(&argv, &mut &stdin[..], &mut err).unwrap();
    (code, String::from_utf8(err).unwrap())
}

fn log(rig: &Rig, name: &str) -> Vec<String> {
    fs::read_to_string(rig.root.join(name)).unwrap_or_default().lines().map(str::to_owned).collect()
}

const PREFIX: &str = "-o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=QUIET ledger-host";

#[test]
fn threads_client_spools_unreachable_writes_and_flushes_them_in_order_riding_the_host_master_while_the_snapshot_pull_keeps_its_own_connection() {
    let mut rig = rig(&[("XDG_RUNTIME_DIR", "/run/user/7")]);
    let riding = format!("-o ControlMaster=no -o ControlPath=/run/user/7/north-ssh-%C -o ServerAliveInterval=5 -o ServerAliveCountMax=2 {PREFIX}");
    fs::write(rig.root.join("code"), "255").unwrap();
    assert_eq!(call(&rig, &["job-run", "nightly", "--host", "laptop"], b"").0, 0);
    let (code, err) = call(&rig, &["ingest"], b"{\"type\": \"run\"}\n");
    assert_eq!(code, 0);
    assert!(err.contains("ledger-host unreachable; spooled ingest to "), "{err}");
    assert_eq!(fs::metadata(&rig.client.spool).unwrap().mode() & 0o777, 0o700);
    let packets: Vec<_> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "argv")).collect();
    assert_eq!(packets.len(), 1);
    assert_eq!(fs::metadata(&packets[0]).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::read_to_string(&packets[0]).unwrap(), r#"["job-run", "nightly", "--host", "laptop"]"#);
    let attempts = log(&rig, "ssh.log").len();
    fs::write(rig.root.join("code"), "0").unwrap();
    assert_eq!(call(&rig, &["list"], b"").0, 0);
    assert_eq!(log(&rig, "ssh.log")[attempts..], [
        format!("{riding} north threads job-run nightly --host laptop"),
        format!("{riding} north threads ingest"),
        format!("{riding} env 'THREADS_ALIVE_OWNERS=[]' north threads list"),
        "-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=QUIET ledger-host north threads snapshot-pages".to_owned(),
    ]);
    assert_eq!(log(&rig, "ingest.log"), ["{\"type\": \"run\"}"]);
    let left: Vec<_> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(left, [".lock"]);
    rig.client.spool_drain = Duration::ZERO;
    rig.client.write_spool(b"{\"n\": 1}\n", "test").unwrap();
    let (code, err) = call(&rig, &["ingest"], b"{\"n\": 2}\n");
    assert_eq!(code, 0);
    assert!(err.contains("ledger-host busy; spooled ingest to "), "an ingest past its drain budget spools behind the backlog: {err}");
    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    assert_eq!(log(&rig, "ingest.log")[1..], ["{\"n\": 1}", "{\"n\": 2}"]);
}

fn spooled(rig: &Rig) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&rig.client.spool).map(|entries| entries.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| !name.starts_with('.')).collect()).unwrap_or_default();
    names.sort();
    names
}

/// At 3:40 PM on 2026-11-06 every fleet launch's 2 s `holder` read sent the spooled usage batches first, each holding the ledger host 4 to 5 s, so the read timed out and the claim check was skipped.
#[test]
fn a_claim_leaves_the_spool_to_a_detached_flush_that_sends_it_one_flusher_at_a_time_and_waits_out_a_records_database_another_writer_holds() {
    let mut rig = rig(&[]);
    let started = rig.root.join("background.log");
    rig.client.background = Some(vec!["/bin/sh".into(), "-c".into(), format!("printf '%s\\n' \"$*\" >> '{}'", started.display()).into(), "north".into(), "threads".into()]);
    rig.client.write_spool(b"{\"n\": 1}\n", "test").unwrap();
    assert_eq!(call(&rig, &["claim", "north#63", "--by", "s"], b"").0, 0);
    assert_eq!(log(&rig, "ssh.log"), [format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads claim 'north#63' --by s")], "the claim sent the spool before itself");
    let mut flush = String::new();
    north_base::platform::wait::until(std::time::Duration::from_secs(60), || {
        flush = fs::read_to_string(&started).unwrap_or_default();
        flush.contains("spool-flush ledger-host") && flush.contains("snapshot-pull")
    });
    assert!(flush.contains("spool-flush ledger-host") && flush.contains("snapshot-pull"), "the detached starts were not a spool-flush and a snapshot-pull: {flush:?}");
    assert_eq!(spooled(&rig).len(), 1);
    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);

    let spool_rig = self::rig(&[]);
    let records = spool::records_path(&spool_rig.client.spool);
    let (held, holding) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let db = rusqlite::Connection::open(records).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        held.send(()).unwrap();
        north_base::platform::wait::until(Duration::from_secs(5), || false);
        db.execute_batch("COMMIT").unwrap();
    });
    holding.recv().unwrap();
    spool_rig.client.write_spool(b"{\"n\": 2}\n", "test").unwrap();
    assert_eq!(call(&spool_rig, &["spool-flush", "ledger-host"], b"").0, 0);
    holder.join().unwrap();
    assert_eq!(log(&spool_rig, "ingest.log"), ["{\"n\": 2}"]);

    fs::write(spool_rig.root.join("hold"), "").unwrap();
    spool_rig.client.write_spool(b"{\"n\": 3}\n", "test").unwrap();
    let (ended, returned) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..7 {
            let ended = ended.clone();
            let spool_rig = &spool_rig;
            scope.spawn(move || ended.send(call(spool_rig, &["spool-flush", "ledger-host"], b"").0).unwrap());
        }
        let mut codes: Vec<i32> = (0..6).map(|_| returned.recv_timeout(Duration::from_secs(60)).expect("six flushers return while the seventh sends")).collect();
        fs::write(spool_rig.root.join("go"), "").unwrap();
        codes.push(returned.recv_timeout(Duration::from_secs(60)).unwrap());
        assert_eq!(codes, [0; 7]);
    });
    assert_eq!(log(&spool_rig, "ingest.log")[1..], ["{\"n\": 3}"]);
}

/// On 2026-11-07 every usage ingest forced a snapshot pull, so whiterabbit and gex45 pulled about 3 times a minute and
/// nexus rebuilt its 600 MB ledger image for each at 10 to 15 s of CPU, holding its CPU pressure over 10% (north#800).
/// On 2026-11-13 gex45's one-minute usage sample, failing to open its snapshot, forced a pull that demanded a fresh image every minute,
/// so nexus rebuilt its 1.1 GB image each minute for a pull the unit's 55 s timeout killed before it landed (north#1054).
#[test]
fn a_second_usage_ingest_inside_five_minutes_pulls_no_second_snapshot_and_a_reader_refresh_takes_the_kept_image() {
    let rig = rig(&[]);
    let usage = b"{\"type\": \"usage\", \"n\": 1}\n";
    assert_eq!(call(&rig, &["ingest"], usage).0, 0);
    assert_eq!(call(&rig, &["ingest"], usage).0, 0);
    assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.ends_with("north threads snapshot-pages")).count(), 1, "{:?}", log(&rig, "ssh.log"));
    let asked = |refresh: fn(&Client)| {
        refresh(&rig.client);
        fs::read(rig.root.join("manifest")).unwrap()[..snapshot::MAGIC.len()] == snapshot::manifest_bytes(0, &[], true)[..snapshot::MAGIC.len()]
    };
    assert!(asked(Client::refresh_mirrored), "a reader's refresh demanded a fresh image, which the ledger host rebuilds for every pull");
    assert!(!asked(Client::refresh_mirrored_now), "the filing guard's refresh took a kept image that can predate the hold it looks for");
}

const LOCK_PATIENCE: Duration = Duration::from_secs(3600);

fn ledger_host(wait: Duration) -> (tempfile::TempDir, Client) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let variables = BTreeMap::from([
        ("HOME".to_owned(), root.display().to_string()),
        ("THREADS_DB".to_owned(), root.join("threads.db").display().to_string()),
    ]);
    let mut client = Client::new(root, variables, vec!["ssh".into()], unread_fleet());
    client.ingest_wait = wait;
    (directory, client)
}

fn batch(id: &str) -> Vec<u8> {
    format!(
        "{{\"id\":\"{id}\",\"role\":\"worker\",\"tier\":\"medium\",\"status\":\"finished\",\"outcome\":\"done\",\"started\":\"2026-10-26T11:00:00Z\",\"ended\":\"2026-10-26T11:05:00Z\",\"actual_min\":5,\"brief\":\"Item: makgora#459 x\\nCategory: feature\"}}\n"
    ).into_bytes()
}

fn recorded(root: &Path) -> Vec<String> {
    let db = rusqlite::Connection::open(root.join("threads.db")).unwrap();
    let mut statement = db.prepare("SELECT agent FROM runs ORDER BY agent").unwrap();
    statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
}

#[test]
fn an_ingest_stalled_past_its_hold_cap_is_stopped_uncommitted_with_the_retry_code_and_its_retry_records_each_row_once_and_one_refused_names_the_lock_holder() {
    let (directory, mut client) = ledger_host(LOCK_PATIENCE);
    client.ingest_hold = Duration::from_secs(1);
    let writer = rusqlite::Connection::open(directory.path().join("threads.db")).unwrap();
    writer.execute_batch("PRAGMA journal_mode = WAL; BEGIN IMMEDIATE").unwrap();
    let mut err = Vec::new();
    let code = client.main(&["ingest".to_owned()], &mut &[batch("one"), batch("two")].concat()[..], &mut err).unwrap();
    assert_eq!(code, ingest_lock::RETRY, "{}", String::from_utf8_lossy(&err));
    assert_eq!(String::from_utf8(err).unwrap(), "NorthThreads: ingest held the ledger past 1 s; stopped it uncommitted, retry\n");
    assert!(ingest_lock::acquire(&directory.path().join("ingest.lock"), Duration::ZERO).unwrap().is_some());
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    let lines = logged(directory.path());
    assert_eq!((lines[0]["exit"].as_i64(), lines[0]["held_ms"].as_i64().is_some_and(|held| held >= 1000)), (Some(i64::from(ingest_lock::RETRY)), true), "{lines:?}");
    for _ in 0..2 {
        assert_eq!(client.main(&["ingest".to_owned()], &mut &[batch("one"), batch("two")].concat()[..], &mut Vec::new()).unwrap(), 0);
    }
    assert_eq!(recorded(directory.path()), ["one", "two"]);
    let lock = directory.path().join("ingest.lock");
    let mut wrapper = std::process::Command::new("flock").arg("-o").arg(&lock).args(["sleep", "30"]).spawn().unwrap();
    assert!(crate::platform::wait::until(Duration::from_secs(10), || ingest_lock::holder(&lock).is_some_and(|holder| holder.held >= Duration::from_secs(1))), "the wrapper never held the lock");
    client.ingest_wait = Duration::from_millis(100);
    let mut err = Vec::new();
    assert_eq!(client.main(&["ingest".to_owned()], &mut &batch("three")[..], &mut err).unwrap(), ingest_lock::RETRY);
    wrapper.kill().unwrap();
    wrapper.wait().unwrap();
    let said = String::from_utf8(err).unwrap();
    assert!(said.contains(&format!("pid {} `flock", wrapper.id())) && said.contains("sleep 30`, holding it for "), "the refusal did not name the lock holder: {said}");
    assert!(logged(directory.path()).last().unwrap()["holder"].as_str().is_some_and(|holder| holder.contains("sleep 30")), "the ingest log did not name the lock holder");
}

fn logged(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join("ingest.log")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}
