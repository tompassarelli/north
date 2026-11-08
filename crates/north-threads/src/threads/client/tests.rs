use super::*;
use serde_json::Value;
use std::os::unix::fs::MetadataExt;
use std::fs::File;
use std::time::Instant;
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
[ -f "$ROOT/sleep" ] && exec sleep "$(cat "$ROOT/sleep")"
case "$last" in
  *ingest) cat >> "$ROOT/ingest.log"; [ -f "$ROOT/ingest-code" ] && exit "$(cat "$ROOT/ingest-code")" ;;
  "north threads snapshot-pages") cat >/dev/null; cat "$ROOT/pages" ;;
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
    snapshot::serve(&ledger, false, &mut &snapshot::manifest_bytes(0, &[])[..], &mut pages).unwrap();
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
fn threads_client_spools_unreachable_writes_and_flushes_them_in_order() {
    let mut rig = rig(&[]);
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
        format!("{PREFIX} north threads job-run nightly --host laptop"),
        format!("{PREFIX} north threads ingest"),
        format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads list"),
        "-o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=QUIET -o ControlPath=none ledger-host north threads snapshot-pages".to_owned(),
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

#[test]
fn a_stalled_ledger_host_returns_a_claim_as_outcome_unknown_within_its_bound_and_never_replays_it() {
    let mut rig = rig(&[]);
    rig.client.ssh_wait = Duration::from_millis(300);
    fs::write(rig.root.join("sleep"), "30").unwrap();
    let begun = Instant::now();
    assert_eq!(call(&rig, &["claim", "wisp#99", "--by", "stall"], b""), (TIMED_OUT_STATUS, "NorthThreads: ledger-host timed out; remote outcome unknown\n".to_owned()));
    assert!(begun.elapsed() < Duration::from_secs(5), "{:?}", begun.elapsed());
    assert_eq!(log(&rig, "ssh.log"), [format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads claim 'wisp#99' --by stall")]);
    assert!(!rig.client.spool.exists(), "a timed-out claim was queued for replay");
}

fn spooled(rig: &Rig) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&rig.client.spool).map(|entries| entries.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| !name.starts_with('.')).collect()).unwrap_or_default();
    names.sort();
    names
}

#[test]
fn a_spooled_batch_whose_flush_times_out_stays_spooled_and_is_sent_again_once_the_host_answers() {
    let mut rig = rig(&[]);
    rig.client.ssh_wait = Duration::from_millis(300);
    let kept = rig.client.write_spool(b"{\"n\": 1}\n", "test").unwrap();
    fs::write(rig.root.join("sleep"), "30").unwrap();
    let (_, err) = call(&rig, &["claim", "north#63"], b"");
    assert!(!err.contains("refused"), "{err}");
    assert_eq!(spooled(&rig), [kept.file_name().unwrap().to_string_lossy().into_owned()], "a timed-out flush is neither .failed nor dropped");
    fs::remove_file(rig.root.join("sleep")).unwrap();
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);
}

/// At 3:40 PM on 2026-11-06 every fleet launch's 2 s `holder` read sent the spooled usage batches first, each holding the ledger host 4 to 5 s, so the read timed out and the claim check was skipped.
#[test]
fn a_claim_leaves_the_spool_to_a_detached_flush_and_that_flush_sends_it() {
    let mut rig = rig(&[]);
    let started = rig.root.join("background.log");
    rig.client.background = Some(vec!["/bin/sh".into(), "-c".into(), format!("printf '%s\\n' \"$*\" >> '{}'", started.display()).into(), "north".into(), "threads".into()]);
    rig.client.write_spool(b"{\"n\": 1}\n", "test").unwrap();
    assert_eq!(call(&rig, &["claim", "north#63", "--by", "s"], b"").0, 0);
    assert_eq!(log(&rig, "ssh.log"), [format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads claim 'north#63' --by s")], "the claim sent the spool before itself");
    let begun = Instant::now();
    while !fs::read_to_string(&started).unwrap_or_default().contains("spool-flush ledger-host") {
        assert!(begun.elapsed() < Duration::from_secs(5), "no detached spool-flush started: {:?}", fs::read_to_string(&started));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(spooled(&rig).len(), 1);
    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);
}

/// On 2026-11-07 every usage ingest forced a snapshot pull, so whiterabbit and gex45 pulled about 3 times a minute and
/// nexus rebuilt its 600 MB ledger image for each at 10 to 15 s of CPU, holding its CPU pressure over 10% (north#800).
#[test]
fn a_second_usage_ingest_inside_five_minutes_pulls_no_second_snapshot() {
    let rig = rig(&[]);
    let usage = b"{\"type\": \"usage\", \"n\": 1}\n";
    assert_eq!(call(&rig, &["ingest"], usage).0, 0);
    assert_eq!(call(&rig, &["ingest"], usage).0, 0);
    assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.ends_with("north threads snapshot-pages")).count(), 1, "{:?}", log(&rig, "ssh.log"));
}

fn stalled_rig() -> Rig {
    let mut rig = rig(&[]);
    rig.client.ssh_wait = Duration::from_millis(300);
    fs::write(rig.root.join("sleep"), "30").unwrap();
    rig
}

#[test]
fn an_ingest_or_agent_event_that_times_out_is_spooled_not_refused_and_sent_again_once_the_host_answers() {
    let rig = stalled_rig();
    let (code, err) = call(&rig, &["ingest"], b"{\"n\": 1}\n");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("ledger-host timed out; spooled ingest to ") && !err.contains("refused"), "{err}");
    assert_eq!(spooled(&rig).iter().filter(|name| name.ends_with(".jsonl")).count(), 1);
    fs::remove_file(rig.root.join("sleep")).unwrap();
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);

    let rig = stalled_rig();
    let event = ["agent-event", "--session", "s", "--kind", "note", "--key", "k1"];
    let (code, err) = call(&rig, &event, b"");
    assert_eq!(code, 0, "{err}");
    assert_eq!(spooled(&rig).iter().filter(|name| name.ends_with(".argv")).count(), 1, "a timed-out agent-event is kept for replay");
    fs::remove_file(rig.root.join("sleep")).unwrap();
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.ends_with("north threads agent-event --session s --kind note --key k1")).count(), 2);
}

#[test]
fn a_busy_ledger_host_spools_the_batch_keeps_it_queued_and_stays_reachable_for_other_commands() {
    let rig = rig(&[]);
    let spooled = || {
        fs::read_dir(rig.root.join(".local/state/threads/spool")).unwrap()
            .filter_map(|entry| entry.unwrap().path().extension().map(|extension| extension.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
    };
    fs::write(rig.root.join("ingest-code"), ingest_lock::RETRY.to_string()).unwrap();
    let (code, err) = call(&rig, &["ingest"], b"{\"n\": 1}\n");
    assert_eq!(code, 0);
    assert!(err.contains("ledger-host busy; spooled ingest to "), "{err}");
    assert_eq!(spooled(), ["jsonl"]);
    let (code, err) = call(&rig, &["claim", "north#63"], b"");
    assert_eq!(code, 0, "a busy ingest lock is not an unreachable host: {err}");
    assert_eq!(spooled(), ["jsonl"], "a busy batch stays queued, never .failed");
    let (code, err) = call(&rig, &["ingest"], b"{\"n\": 2}\n");
    assert_eq!(code, 0);
    assert!(err.contains("spooled ingest to "), "{err}");
    assert_eq!(spooled(), ["jsonl", "jsonl"]);
    fs::remove_file(rig.root.join("ingest-code")).unwrap();
    assert_eq!(call(&rig, &["ingest"], b"{\"n\": 3}\n").0, 0);
    assert!(spooled().is_empty());
    let sent = log(&rig, "ingest.log");
    assert_eq!(&sent[sent.len() - 3..], ["{\"n\": 1}", "{\"n\": 2}", "{\"n\": 3}"]);
}

#[test]
fn threads_client_reads_fall_back_to_the_snapshot_with_the_stale_line() {
    let rig = rig(&[]);
    fs::write(rig.root.join("code"), "255").unwrap();
    schema::ensure(&rig.client.snapshot).unwrap();
    rusqlite::Connection::open(&rig.client.snapshot).unwrap().execute("INSERT INTO job_runs VALUES ('nightly', 'laptop', '2026-11-05T00:00:00Z', NULL, NULL)", []).unwrap();
    File::options().write(true).open(&rig.client.snapshot).unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000)).unwrap();
    let (code, err) = call(&rig, &["job-runs"], b"");
    assert_eq!(code, 0);
    assert_eq!(err, "STALE: ledger snapshot from 2023-11-14T22:13:20Z; ledger-host unreachable\n");
    let (code, err) = call(&rig, &["claim", "north#63"], b"");
    assert_eq!(code, 255);
    assert_eq!(err, "NorthThreads: ledger-host unreachable; the ledger lives there, so nothing was read or recorded\n");
    let held: i64 = rusqlite::Connection::open(&rig.client.snapshot).unwrap().query_row("SELECT count(*) FROM claims", [], |row| row.get(0)).unwrap();
    assert_eq!(held, 0, "an unreachable host records nothing in the snapshot");
}

/// At 1:10 PM on 2026-11-07 nexus sat at load 8.7 on 4 cores with peer-started `threads recommend` and `threads unowned` each at 85% of a core.
#[test]
fn a_peer_sends_a_recording_free_recommend_nowhere_and_only_writes_to_the_ledger_host() {
    let rig = rig(&[]);
    schema::ensure(&rig.client.snapshot).unwrap();
    call(&rig, &["recommend", "Category: feature"], b"");
    assert_eq!(call(&rig, &["recommend", "Item: north#63\nCategory: feature"], b"").0, 0);
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    let sent: Vec<String> = log(&rig, "ssh.log").into_iter().filter(|line| !line.ends_with("snapshot-pages")).collect();
    assert_eq!(sent.join("\n"), format!("{PREFIX} north threads recommend 'Item: north#63\nCategory: feature'\n{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads claim 'north#63'"));
}

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
fn two_ingests_started_together_run_one_after_the_other_and_each_batch_is_recorded_once() {
    let (directory, client) = ledger_host(Duration::from_secs(30));
    let ingest = |id: &'static str| {
        let client = &client;
        move || {
            let mut err = Vec::new();
            let code = client.main(&["ingest".to_owned()], &mut &batch(id)[..], &mut err).unwrap();
            (code, String::from_utf8(err).unwrap())
        }
    };
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(ingest("one"));
        let second = scope.spawn(ingest("two"));
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!((first.0, second.0), (0, 0), "{first:?} {second:?}");
    let millis = |line: &Value, field: &str| Timestamp::parse_loose(line[field].as_str().unwrap()).unwrap().unix_millis();
    let mut holds: Vec<(i64, i64)> = logged(directory.path()).iter().map(|line| (millis(line, "ended") - line["held_ms"].as_i64().unwrap(), millis(line, "ended"))).collect();
    holds.sort_unstable();
    assert!(holds.len() == 2 && holds[0].1 <= holds[1].0, "the two ingests held the ledger at once: {holds:?}");
    assert_eq!(recorded(directory.path()), ["one", "two"]);
}

#[test]
fn an_ingest_stalled_past_its_hold_cap_is_stopped_uncommitted_with_the_retry_code_and_its_retry_records_each_row_once() {
    let (directory, mut client) = ledger_host(Duration::from_secs(5));
    client.ingest_hold = Duration::from_secs(3);
    let writer = rusqlite::Connection::open(directory.path().join("threads.db")).unwrap();
    writer.execute_batch("PRAGMA journal_mode = WAL; BEGIN IMMEDIATE").unwrap();
    let mut err = Vec::new();
    let begun = Instant::now();
    let code = client.main(&["ingest".to_owned()], &mut &[batch("one"), batch("two")].concat()[..], &mut err).unwrap();
    let took = begun.elapsed();
    assert_eq!(code, ingest_lock::RETRY, "{}", String::from_utf8_lossy(&err));
    assert!(took >= Duration::from_secs(3) && took < Duration::from_secs(30), "{took:?}");
    assert_eq!(String::from_utf8(err).unwrap(), "NorthThreads: ingest held the ledger past 3 s; stopped it uncommitted, retry\n");
    assert!(ingest_lock::acquire(&directory.path().join("ingest.lock"), Duration::ZERO).unwrap().is_some());
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    let lines = logged(directory.path());
    assert_eq!((lines[0]["exit"].as_i64(), lines[0]["held_ms"].as_i64().is_some_and(|held| held < 30_000)), (Some(i64::from(ingest_lock::RETRY)), true), "{lines:?}");
    for _ in 0..2 {
        assert_eq!(client.main(&["ingest".to_owned()], &mut &[batch("one"), batch("two")].concat()[..], &mut Vec::new()).unwrap(), 0);
    }
    assert_eq!(recorded(directory.path()), ["one", "two"]);
}

#[test]
fn a_batch_past_the_chunk_size_is_committed_in_chunks_each_under_its_own_lock_and_logged_apart() {
    let (directory, mut client) = ledger_host(Duration::from_secs(5));
    client.ingest_chunk_rows = 2;
    let batches: Vec<u8> = ["a", "b", "c", "d", "e"].iter().flat_map(|id| batch(id)).collect();
    assert_eq!(client.main(&["ingest".to_owned()], &mut &batches[..], &mut Vec::new()).unwrap(), 0);
    let rows: Vec<u64> = logged(directory.path()).iter().map(|line| line["rows"]["run"].as_u64().unwrap()).collect();
    assert_eq!(rows, [2, 2, 1]);
    assert_eq!(recorded(directory.path()), ["a", "b", "c", "d", "e"]);
}

fn logged(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join("ingest.log")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

