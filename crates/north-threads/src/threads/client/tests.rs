use super::*;
use serde_json::Value;
use std::os::unix::fs::MetadataExt;
use std::fs::File;
use std::collections::BTreeSet;
use crate::foundation::exit::TIMED_OUT_STATUS;

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

const STALL_BOUND: Duration = Duration::from_secs(7);

fn stall_on_event(root: PathBuf, bounds: Arc<std::sync::Mutex<Vec<Duration>>>) -> Await {
    Arc::new(move |child: &mut std::process::Child, bound: Duration| -> io::Result<Option<std::process::ExitStatus>> {
        bounds.lock().unwrap().push(bound);
        let mut exited = None;
        crate::platform::wait::until(Duration::from_secs(3600), || {
            exited = child.try_wait().transpose();
            exited.is_some() || fs::remove_file(root.join("stalled")).is_ok()
        });
        if let Some(result) = exited {
            return result.map(Some);
        }
        child.kill()?;
        child.wait()?;
        Ok(None)
    })
}

fn stall(rig: &mut Rig) -> Arc<std::sync::Mutex<Vec<Duration>>> {
    let bounds = Arc::default();
    rig.client.ssh_wait = STALL_BOUND;
    rig.client.await_child = stall_on_event(rig.root.clone(), Arc::clone(&bounds));
    fs::write(rig.root.join("stall"), "").unwrap();
    bounds
}

fn unstall(rig: &Rig) {
    fs::remove_file(rig.root.join("stall")).unwrap();
}

const PREFIX: &str = "-o ControlPath=none -o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=QUIET ledger-host";

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

#[test]
fn a_stalled_ledger_host_returns_a_claim_as_outcome_unknown_within_its_bound_and_never_replays_it() {
    let mut rig = rig(&[]);
    let bounds = stall(&mut rig);
    assert_eq!(call(&rig, &["claim", "wisp#99", "--by", "stall"], b""), (TIMED_OUT_STATUS, "NorthThreads: ledger-host timed out; remote outcome unknown\n".to_owned()));
    assert_eq!(log(&rig, "ssh.log"), [format!("{PREFIX} env 'THREADS_ALIVE_OWNERS=[]' north threads claim 'wisp#99' --by stall")]);
    assert_eq!(*bounds.lock().unwrap(), [STALL_BOUND], "the call was not bounded by the client's ssh wait");
    assert!(!rig.client.spool.exists(), "a timed-out claim was queued for replay");
}

fn spooled(rig: &Rig) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&rig.client.spool).map(|entries| entries.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| !name.starts_with('.')).collect()).unwrap_or_default();
    names.sort();
    names
}

/// On 2026-11-13 a 5,360-row spooled batch outlasted the ssh bound on every flush and went back to the queue each time, holding every claim and release behind it.
#[test]
fn a_spooled_batch_whose_flush_times_out_is_split_set_aside_after_three_timeouts_and_never_holds_a_claim_behind_it() {
    let mut rig = rig(&[]);
    rig.client.ingest_chunk_rows = 2;
    let kept = rig.client.write_spool(b"{\"n\": 1}\n{\"n\": 2}\n{\"n\": 3}\n", "test").unwrap();
    let stem = kept.file_stem().unwrap().to_string_lossy().into_owned();
    stall(&mut rig);
    let (_, err) = call(&rig, &["claim", "north#63"], b"");
    assert!(!err.contains("refused"), "{err}");
    assert_eq!(spooled(&rig), [format!("{stem}-0000.jsonl"), format!("{stem}-0001.jsonl")], "a timed-out flush is neither .failed nor dropped");
    call(&rig, &["claim", "north#63"], b"");
    let (_, err) = call(&rig, &["claim", "north#63"], b"");
    assert!(err.contains("timed out; it is set aside"), "{err}");
    rig.client.spool_argv(&["release".to_owned(), "north#7".to_owned()], "test").unwrap();
    unstall(&rig);
    let before = log(&rig, "ssh.log").len();
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert_eq!(log(&rig, "ssh.log")[before..before + 2], [format!("{PREFIX} north threads release 'north#7'"), format!("{PREFIX} north threads ingest")], "the release waited behind the ingest batch");
    assert_eq!(spooled(&rig), [format!("{stem}-0000.jsonl")]);
    assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 3}"]);
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
    stall(&mut rig);
    rig
}

#[test]
fn an_ingest_or_agent_event_that_times_out_is_spooled_not_refused_and_sent_again_once_the_host_answers() {
    let rig = stalled_rig();
    let (code, err) = call(&rig, &["ingest"], b"{\"n\": 1}\n");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("ledger-host timed out; spooled ingest to ") && !err.contains("refused"), "{err}");
    assert_eq!(spooled(&rig).iter().filter(|name| name.ends_with(".jsonl")).count(), 1);
    unstall(&rig);
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ingest.log"), ["{\"n\": 1}"]);

    let rig = stalled_rig();
    let event = ["agent-event", "--session", "s", "--kind", "note", "--key", "k1"];
    let (code, err) = call(&rig, &event, b"");
    assert_eq!(code, 0, "{err}");
    assert_eq!(spooled(&rig).iter().filter(|name| name.ends_with(".argv")).count(), 1, "a timed-out agent-event is kept for replay");
    unstall(&rig);
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
    let window = |used: usize| format!("{{\"account\":\"e\",\"kind\":\"window\",\"name\":\"session\",\"type\":\"usage\",\"used_pct\":{used}.0}}");
    for row in 0..120 {
        rig.client.write_spool(format!("{}\n", window(50 + row / 60)).as_bytes(), "test").unwrap();
    }
    let ingests = || log(&rig, "ssh.log").iter().filter(|line| line.ends_with("north threads ingest")).count();
    let before = ingests();
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert_eq!(ingests() - before, 2, "the claim sent the spooled usage rows before itself");
    assert_eq!(spooled().len(), 120);
    assert_eq!(call(&rig, &["ingest"], b"{\"n\": 3}\n").0, 0);
    assert_eq!(ingests() - before, 4, "the usage rows went to the ledger host in more than one ingest");
    assert!(spooled().is_empty());
    let sent = log(&rig, "ingest.log");
    assert_eq!(sent[sent.len() - 5..], ["{\"n\": 1}".to_owned(), "{\"n\": 2}".to_owned(), window(50), window(51), "{\"n\": 3}".to_owned()]);
}

#[test]
fn an_ingest_the_ledger_host_could_not_run_goes_back_to_the_queue() {
    for unrunnable in [126, 127] {
        let rig = rig(&[]);
        fs::write(rig.root.join("ingest-code"), ingest_lock::RETRY.to_string()).unwrap();
        assert_eq!(call(&rig, &["ingest"], b"{\"n\": 1}\n").0, 0);
        fs::write(rig.root.join("ingest-code"), unrunnable.to_string()).unwrap();
        let (code, err) = call(&rig, &["claim", "north#63"], b"");
        assert_eq!(code, 0, "{err}");
        assert!(!err.contains("refused"), "exit {unrunnable} says nothing about the row: {err}");
        fs::remove_file(rig.root.join("ingest-code")).unwrap();
        assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
        assert!(spooled(&rig).is_empty(), "exit {unrunnable} left the row stuck: {:?}", spooled(&rig));
        assert_eq!(log(&rig, "ingest.log").last().map(String::as_str), Some("{\"n\": 1}"));
    }

    let rig = rig(&[]);
    fs::write(rig.root.join("ingest-code"), ingest_lock::RETRY.to_string()).unwrap();
    assert_eq!(call(&rig, &["ingest"], b"{\"n\": 1}\n").0, 0);
    fs::write(rig.root.join("ingest-code"), "1").unwrap();
    let (_, err) = call(&rig, &["claim", "north#63"], b"");
    assert!(err.contains("remote ingest refused"), "{err}");
    fs::remove_file(rig.root.join("ingest-code")).unwrap();
    let sent = log(&rig, "ingest.log").len();
    call(&rig, &["claim", "north#63"], b"");
    assert_eq!(log(&rig, "ingest.log").len(), sent, "a refused row is not sent again");
}

#[test]
fn a_spooled_write_the_ledger_host_refuses_for_good_is_refused_once_naming_its_verb() {
    let rig = rig(&[]);
    fs::write(rig.root.join("code"), "255").unwrap();
    assert_eq!(call(&rig, &["job-run", "nightly", "--host", "laptop"], b"").0, 0);
    assert_eq!(spooled(&rig).len(), 1);
    fs::write(rig.root.join("code"), "0").unwrap();
    fs::write(rig.root.join("refusal"), "remote-serve: refused for gex45: command job-run is not on the allowlist\n").unwrap();
    let (_, err) = call(&rig, &["claim", "north#63"], b"");
    assert!(err.contains("will not run `job-run`"), "{err}");
    let attempts = log(&rig, "ssh.log").iter().filter(|line| line.contains("threads job-run")).count();
    assert_eq!(attempts, 2, "one unreachable send and one refused replay");

    let now = Timestamp::now();
    let faults = north_base::lifecycle::Store::open(&spool::records_path(&rig.client.spool)).unwrap().faults(now).unwrap();
    let refused = faults.iter().find(|fault| fault.machine == "threads-spool").expect("a refused spool write is reported");
    assert!(refused.cause.contains("`job-run`") && refused.cause.contains("not on the allowlist"), "{:?}", refused.cause);
    assert_eq!(refused.state, "refused");

    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.contains("threads job-run")).count(), attempts, "a write refused for good is not sent again");
}

/// On 2026-11-08 the 29 `release` writes gex45's spool held as refused, each by an exit 126 before such an exit was retried, kept the state-machines flow alarmed for their 7-day keep.
#[test]
fn a_refused_claim_release_row_is_resent_once_its_claim_frees() {
    let mut rig = rig(&[]);
    let start = 1_800_000_000;
    let now = Arc::new(std::sync::atomic::AtomicI64::new(start));
    let clock = Arc::clone(&now);
    rig.client.clock = Arc::new(move || Timestamp::from_unix_seconds(clock.load(std::sync::atomic::Ordering::SeqCst)));
    let at = |seconds: i64| now.store(start + seconds, std::sync::atomic::Ordering::SeqCst);
    let release = |issue: &str, session: &str| ["release", issue, "--by", session, "--held-by", session].map(str::to_owned);
    rig.client.spool_argv(&release("tompassarelli/smashcraft#334", "sc-334"), "the caller's bound passed before the ledger host answered").unwrap();
    rig.client.spool_argv(&release("tompassarelli/smashcraft#315", "sc-315"), "the caller's bound passed before the ledger host answered").unwrap();
    fs::write(rig.root.join("refusal"), "remote-serve: refused for gex45: command release is not on the allowlist\n").unwrap();
    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    rig.client.spool_argv(&release("tompassarelli/smashcraft#324", "sc-324"), "the caller's bound passed before the ledger host answered").unwrap();
    let ledger = spool::records_path(&rig.client.spool);
    let mut spool = spool::Spool::open(&rig.client.spool, &ledger, Timestamp::from_unix_seconds(start)).unwrap();
    let legacy = spool.queued(Timestamp::from_unix_seconds(start)).unwrap().remove(0);
    assert!(spool.take(&legacy, Timestamp::from_unix_seconds(start)).unwrap());
    spool.refuse(&legacy, "the ledger host exited 126", Timestamp::from_unix_seconds(start)).unwrap();
    let alarmed = |rig: &Rig, seconds: i64| north_base::lifecycle::Store::open(&spool::records_path(&rig.client.spool)).unwrap().faults(Timestamp::from_unix_seconds(start + seconds)).unwrap().into_iter().filter(|fault| fault.machine == "threads-spool").count();
    assert_eq!(alarmed(&rig, 1), 3);
    let sends = |rig: &Rig| log(rig, "ssh.log").iter().filter(|line| line.contains("threads release")).count();
    let refused = sends(&rig);

    fs::remove_file(rig.root.join("refusal")).unwrap();
    at(10 * 60);
    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    assert_eq!(sends(&rig), refused, "a refusal under an hour old is not asked again");

    at(61 * 60);
    assert_eq!(call(&rig, &["spool-flush", "ledger-host"], b"").0, 0);
    assert_eq!(sends(&rig), refused + 3, "one write asks, and its delivery sends the other two");
    assert!(spooled(&rig).is_empty(), "{:?}", spooled(&rig));
    assert_eq!(alarmed(&rig, 62 * 60), 0);
}

#[test]
fn a_spooled_verb_the_host_could_not_run_at_all_stays_queued_for_the_next_flush() {
    let rig = rig(&[]);
    fs::write(rig.root.join("code"), "255").unwrap();
    assert_eq!(call(&rig, &["job-run", "nightly", "--host", "laptop"], b"").0, 0);
    fs::write(rig.root.join("code"), "126").unwrap();
    let (_, err) = call(&rig, &["claim", "north#63"], b"");
    assert!(!err.contains("will not run"), "a 126 with no refusal says the host could not run the binary, not that the verb is refused: {err}");
    assert_eq!(spooled(&rig).len(), 1);
    fs::write(rig.root.join("code"), "0").unwrap();
    assert_eq!(call(&rig, &["claim", "north#63"], b"").0, 0);
    assert!(spooled(&rig).is_empty());
    assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.contains("threads job-run")).count(), 3, "the unreachable send, the 126 that could not run it, and the replay");
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
fn two_ingests_started_together_run_one_after_the_other_and_each_batch_is_recorded_once() {
    let (directory, client) = ledger_host(LOCK_PATIENCE);
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
}

#[test]
fn a_batch_past_the_chunk_size_is_committed_in_chunks_each_under_its_own_lock_and_logged_apart() {
    let (directory, mut client) = ledger_host(LOCK_PATIENCE);
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


/// On 2026-11-10 two new peer hosts never got a first snapshot: the ~780 MB pull outlasted its 80 s and every retry began again from nothing (north#1076).
#[test]
fn a_first_snapshot_pull_cut_short_keeps_its_pages_and_the_next_pull_finishes_it() {
    let rig = rig(&[]);
    let pages = fs::read(rig.root.join("pages")).unwrap();
    let cut = rig.root.join("cut");
    fs::write(&cut, &pages[..pages.len() - 40]).unwrap();
    let snapshot = rig.root.join("first.db");
    let serving = |file: &Path| vec![OsString::from("/bin/sh"), "-c".into(), "cat >/dev/null; cat \"$1\"".into(), "ssh".into(), file.into()];
    assert!(snapshot::pull(&serving(&cut), &snapshot, Duration::from_secs(600), false).is_err());
    assert!(!snapshot.exists() && rig.root.join("first.db.partial").exists());
    snapshot::pull(&serving(&rig.root.join("pages")), &snapshot, Duration::from_secs(600), false).unwrap();
    assert!(snapshot.exists() && !rig.root.join("first.db.partial").exists());
}

/// On 2026-11-11 each peer pull made the ledger host rebuild its 800 MB image, 20 CPU seconds and 1.6 GB a time, four times in a quarter hour on a 3-core host (north#1036).
#[test]
fn pulls_that_accept_a_recent_image_share_one_build_and_pulls_that_demand_a_fresh_one_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("ledger.db");
    let connection = rusqlite::Connection::open(&ledger).unwrap();
    connection.execute_batch("CREATE TABLE t(v); INSERT INTO t VALUES (1);").unwrap();
    let served = |recent: bool| {
        let mut pages = Vec::new();
        snapshot::serve(&ledger, false, &mut &snapshot::manifest_bytes(0, &[], recent)[..], &mut pages).unwrap();
        pages
    };
    let first = served(true);
    connection.execute_batch("INSERT INTO t VALUES (2);").unwrap();
    assert_eq!(served(true), first, "a recent pull rebuilt the image");
    assert_ne!(served(false), first, "a fresh pull was answered from the old image");
    assert_ne!(served(true), first, "the fresh build did not replace the kept image");
}

#[test]
fn a_pull_that_demands_a_fresh_image_of_an_unchanged_ledger_is_answered_without_a_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("ledger.db");
    let connection = rusqlite::Connection::open(&ledger).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(v); INSERT INTO t VALUES (1);").unwrap();
    let serve = || snapshot::serve(&ledger, false, &mut &snapshot::manifest_bytes(0, &[], false)[..], &mut Vec::new()).unwrap();
    let built = || std::fs::metadata(directory.path().join("ledger.db.served-full")).unwrap().modified().unwrap();
    for file in ["ledger.db", "ledger.db-wal"] {
        std::fs::File::options().write(true).open(directory.path().join(file)).unwrap().set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
    }
    serve();
    let first = built();
    serve();
    assert_eq!(built(), first, "an unchanged ledger was rebuilt");
    connection.execute_batch("INSERT INTO t VALUES (2);").unwrap();
    serve();
    assert_ne!(built(), first, "a changed ledger was answered from the old image");
}
