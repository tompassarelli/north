use std::path::PathBuf;

use rusqlite::Connection;

use super::run;
use crate::threads::fake::Recorded;
use crate::threads::verb::Context;
use crate::foundation::clock::Timestamp;

const SCHEMA: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL, actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE TABLE job_runs(name TEXT NOT NULL, host TEXT NOT NULL, started TEXT NOT NULL, finished TEXT, rc INTEGER, PRIMARY KEY(name, host, started)) STRICT;
CREATE TABLE job_queue(id INTEGER PRIMARY KEY, placement TEXT NOT NULL CHECK(length(placement) > 0), item TEXT, cmd TEXT NOT NULL CHECK(json_valid(cmd)), state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','taken','done','failed','dropped','requeued')), queued TEXT NOT NULL, taken TEXT, finished TEXT, result TEXT) STRICT;";
const NOW: &str = "2026-10-27T12:00:00Z";

struct Ledger {
    _directory: tempfile::TempDir,
    path: PathBuf,
    outside: Recorded,
    no_native: Option<&'static str>,
}

fn ledger() -> Ledger {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA).unwrap();
    Ledger { _directory: directory, path, outside: Recorded::default(), no_native: None }
}

impl Ledger {
    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    fn sql(&self, query: &str) -> String {
        self.db().query_row(query, [], |row| row.get::<_, String>(0)).unwrap()
    }

    fn feed(&self, argv: &str, stdin: &str) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: self.no_native, outside: &self.outside, now: Timestamp::parse(NOW).unwrap() };
        let argv: Vec<String> = shell_words(argv);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &context, &mut stdin.as_bytes(), &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn run(&self, argv: &str) -> (i32, String, String) {
        self.feed(argv, "")
    }

    fn out(&self, argv: &str) -> String {
        let (code, out, err) = self.run(argv);
        assert_eq!((code, err.as_str()), (0, ""), "{argv}");
        out
    }
}

fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut quoted = false;
    let mut word = String::new();
    for character in text.chars() {
        match character {
            '\'' => quoted = !quoted,
            ' ' if !quoted => words.extend((!word.is_empty()).then(|| std::mem::take(&mut word))),
            _ => word.push(character),
        }
    }
    words.extend((!word.is_empty()).then_some(word));
    words
}

#[test]
fn two_concurrent_takes_claim_the_one_job_once_and_its_result_is_commented_on_its_issue_once() {
    let rig = ledger();
    let id = rig.out("queue add laptop smashcraft#3 -- echo hi");
    let id = id.trim();
    let path = rig.path.as_path();
    let takes: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2).map(|_| scope.spawn(move || ledger_take(path))).collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect()
    });
    let wanted = format!("{{\"id\": {id}, \"placement\": \"laptop\", \"item\": \"smashcraft#3\", \"cmd\": [\"echo\", \"hi\"]}}\n");
    assert_eq!(takes.iter().filter(|out| **out == wanted).count(), 1, "{takes:?}");
    assert!(takes.iter().any(String::is_empty), "{takes:?}");
    assert_eq!(rig.out(&format!("queue msg {id} done said hi")), format!("job {id} done\n"));
    let (code, _, err) = rig.run(&format!("queue msg {id} done again"));
    assert_eq!((code, err), (1, format!("NorthThreads: queue msg: job {id} is not taken\n")));
    assert_eq!(*rig.outside.calls.borrow(), [["issue", "comment", "3", "-R", "tompassarelli/smashcraft", "--body", format!("Job {id} done:\n\nsaid hi").as_str()].map(str::to_owned).to_vec()]);
    assert_eq!(rig.sql(&format!("SELECT state || '|' || result FROM job_queue WHERE id = {id}")), "done|said hi");
}

fn ledger_take(path: &std::path::Path) -> String {
    let outside = Recorded::default();
    let context = Context { database: path, actor: "tester".into(), no_native: None, outside: &outside, now: Timestamp::parse(NOW).unwrap() };
    let mut out = Vec::new();
    assert_eq!(run(&shell_words("queue take laptop"), &context, &mut &b""[..], &mut out, &mut Vec::new()), 0);
    String::from_utf8(out).unwrap()
}

#[test]
fn a_pool_job_goes_to_the_first_runner_that_takes_it_and_a_laptop_only_take_leaves_pool_jobs_alone() {
    let rig = ledger();
    let first = rig.out("queue add pool - -- echo p1").trim().to_owned();
    let second = rig.out("queue add pool - -- echo 'p 2'").trim().to_owned();
    assert!(rig.out("queue take nexus,pool").starts_with(&format!("{{\"id\": {first}, \"placement\": \"pool\", \"item\": null,")));
    rig.out(&format!("job-run queue-{first} --host nexus --started 2026-10-12T10:02:00Z"));
    assert_eq!(rig.out("queue take laptop"), "");
    assert!(rig.out("queue take laptop,pool").starts_with(&format!("{{\"id\": {second},")));
    rig.out(&format!("job-run queue-{second} --host whiterabbit --started 2026-10-12T10:03:00Z"));
    rig.db().execute("UPDATE job_queue SET queued = '2026-10-27T11:00:00Z', taken = '2026-10-27T11:30:30Z'", []).unwrap();
    assert_eq!(
        rig.out("queue"),
        format!(
            "id  placement  item  state  queued                wait  finished  job_run                                cmd\n\
             {first}   pool       -     taken  2026-10-27T11:00:00Z  30m   -         nexus 2026-10-12T10:02:00Z rc=-        echo p1\n\
             {second}   pool       -     taken  2026-10-27T11:00:00Z  30m   -         whiterabbit 2026-10-12T10:03:00Z rc=-  echo 'p 2'\n"
        )
    );
    assert_eq!(
        rig.out("queue --json"),
        format!("[{{\"id\": {first}, \"placement\": \"pool\", \"state\": \"taken\", \"queued\": \"2026-10-27T11:00:00Z\", \"taken\": \"2026-10-27T11:30:30Z\", \"finished\": null, \"host\": \"nexus\"}}, {{\"id\": {second}, \"placement\": \"pool\", \"state\": \"taken\", \"queued\": \"2026-10-27T11:00:00Z\", \"taken\": \"2026-10-27T11:30:30Z\", \"finished\": null, \"host\": \"whiterabbit\"}}]\n")
    );
}

#[test]
fn a_dropped_job_keeps_its_reason_and_a_ledger_note_and_taken_or_dropped_jobs_are_refused() {
    let rig = ledger();
    let taken = rig.out("queue add dropq - -- echo d0").trim().to_owned();
    rig.out("queue take dropq");
    let queued = rig.out("queue add dropq smashcraft#3 -- echo d1").trim().to_owned();
    assert_eq!(rig.out(&format!("queue --by hand drop {queued} superseded by hand")), format!("job {queued} dropped\n"));
    assert_eq!(rig.run(&format!("queue drop {queued} again")), (1, String::new(), format!("NorthThreads: queue drop: job {queued} is dropped, not queued\n")));
    assert_eq!(rig.run(&format!("queue drop {taken} busy")), (1, String::new(), format!("NorthThreads: queue drop: job {taken} is taken, not queued\n")));
    assert_eq!(rig.run("queue drop 99 gone"), (1, String::new(), "NorthThreads: queue drop: job 99 is unknown, not queued\n".into()));
    assert_eq!(rig.out("queue take dropq"), "");
    assert_eq!(rig.sql(&format!("SELECT state || '|' || result || '|' || finished FROM job_queue WHERE id = {queued}")), format!("dropped|superseded by hand|{NOW}"));
    assert_eq!(rig.sql("SELECT item || '|' || actor || '|' || data FROM events WHERE kind = 'note'"), format!("smashcraft#3|hand|{{\"job\": {queued}, \"text\": \"job {queued} dropped: superseded by hand\"}}"));
    assert_eq!(rig.sql(&format!("SELECT state FROM job_queue WHERE id = {taken}")), "taken");
}

#[test]
fn a_requeued_job_goes_back_to_its_place_and_a_native_job_result_is_a_ledger_note_not_a_comment() {
    let rig = ledger();
    let first = rig.out("queue add laptop ops#4 -- echo a").trim().to_owned();
    let second = rig.out("queue add laptop - -- echo b").trim().to_owned();
    rig.out("queue take laptop");
    assert_eq!(rig.out(&format!("queue msg {first} requeued rc=143")), format!("job {first} requeued\n"));
    assert!(rig.out("queue take laptop").starts_with(&format!("{{\"id\": {first},")));
    assert_eq!(rig.sql(&format!("SELECT state FROM job_queue WHERE id = {second}")), "queued");
    rig.out(&format!("queue msg {first} failed -- rc=1"));
    assert!(rig.outside.calls.borrow().is_empty());
    assert_eq!(
        rig.sql("SELECT group_concat(item || '|' || data, ';') FROM events"),
        format!("ops#4|{{\"text\": \"job {first} requeued: rc=143\"}};ops#4|{{\"text\": \"job {first} failed: rc=1\"}}")
    );
}

#[test]
fn a_secret_in_a_result_is_never_posted_and_native_items_are_hidden_from_callers_that_may_not_read_them() {
    let mut rig = ledger();
    rig.outside.secret = Some("ghp_");
    let id = rig.out("queue add laptop wisp#2 -- true").trim().to_owned();
    rig.out("queue add laptop life#1 -- true");
    rig.out("queue take laptop");
    let (code, _, err) = rig.run(&format!("queue msg {id} done token ghp_x"));
    assert_eq!((code, err.as_str()), (1, "NorthThreads: gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored\n"));
    assert!(rig.outside.calls.borrow().is_empty());
    rig.no_native = Some("THREADS_NO_NATIVE is set (agy)");
    assert!(rig.out("queue").lines().nth(2).unwrap().starts_with("2   laptop     native  queued"), "{}", rig.out("queue"));
    assert_eq!(rig.run("queue add laptop life#2 -- true"), (1, String::new(), "NorthThreads: native items are not readable here: THREADS_NO_NATIVE is set (agy)\n".into()));
}

#[test]
fn a_job_run_records_its_start_then_its_finish_and_refuses_half_a_finish() {
    let rig = ledger();
    rig.out("job-run nightly --host nexus --started 2026-10-26T01:00:00Z");
    rig.out("job-run nightly --host nexus --started 2026-10-26T01:00:00Z --finished 2026-10-26T01:05:00Z --rc 0");
    rig.out("job-run nightly --host nexus --started 2026-10-26T01:00:00Z");
    rig.out("job-run nightly --host nexus --started=2026-10-27T01:00:00Z --finished 2026-10-27T01:09:00Z --rc 3");
    rig.out("job-run nightly --host nexus --started 2026-10-27T02:00:00Z");
    assert_eq!(rig.run("job-run nightly --host nexus --started 2026-10-27T03:00:00Z --rc 0"), (1, String::new(), "NorthThreads: job-run: --finished and --rc go together\n".into()));
    assert_eq!(rig.run("job-run nightly --started x").0, 2);
    assert_eq!(
        rig.out("job-runs"),
        "name     host   runs  failed  open  last_started          last_rc\nnightly  nexus  3     1       1     2026-10-27T02:00:00Z  -\n"
    );
    assert_eq!(
        rig.out("job-runs --json"),
        "[{\"name\": \"nightly\", \"host\": \"nexus\", \"first_started\": \"2026-10-26T01:00:00Z\", \"last_started\": \"2026-10-27T02:00:00Z\", \"last_ok\": \"2026-10-26T01:05:00Z\", \"last_rc\": null, \"open_since\": \"2026-10-27T02:00:00Z\"}]\n"
    );
}

#[test]
fn a_follows_box_queues_its_brief_and_each_release_step_happens_once() {
    let rig = ledger();
    let brief = "Item: north#9\nFollows: `Land the fix (north#8)`\n";
    assert_eq!(rig.feed("queue follow nixos-config#9 --tier worker --parent lead", "no line\n"), (1, String::new(), "NorthThreads: queue follow: the brief on stdin has no Follows: line\n".into()));
    let id = rig.feed("queue follow nixos-config#9 --tier worker --parent lead --repo=", brief).1.trim().to_owned();
    let listed = format!("[{{\"id\": {id}, \"item\": \"firn#9\", \"state\": \"queued\", \"queued\": \"{NOW}\", \"taken\": null, \"alerted\": null, \"repo\": \"firn\", \"tier\": \"worker\", \"parent\": \"lead\", \"follows\": \"Land the fix (north#8)\", \"brief\": \"Item: north#9\\nFollows: `Land the fix (north#8)`\\n\"}}]\n");
    assert_eq!(rig.out("queue follow-list"), listed);
    assert!(rig.out("queue").contains("follows Land the fix (north#8) on worker"));
    assert_eq!(rig.run(&format!("queue follow-started {id}")).0, 1);
    rig.out(&format!("queue follow-release {id}"));
    assert_eq!(rig.run(&format!("queue follow-release {id}")), (1, String::new(), format!("NorthThreads: queue follow-release: box {id} is not an open follows box in state queued\n")));
    rig.out(&format!("queue follow-alert {id} -- no lead answered"));
    assert_eq!(rig.run(&format!("queue follow-alert {id} again")).0, 1);
    rig.out(&format!("queue follow-started {id}"));
    assert_eq!(rig.out("queue follow-list"), "[]\n");
    assert_eq!(rig.sql(&format!("SELECT state || '|' || result || '|' || finished FROM job_queue WHERE id = {id}")), format!("done|no lead answered|{NOW}"));
}
