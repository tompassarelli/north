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
    fn feed(&self, argv: &str, stdin: &str) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: self.no_native, outside: &self.outside, now: Timestamp::parse(NOW).unwrap(), replica: false };
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
fn a_secret_in_a_result_is_never_posted_and_native_items_are_hidden_from_callers_that_may_not_read_them() {
    let mut rig = ledger();
    rig.outside.secret = Some("ghp_");
    let id = rig.out("queue add laptop wisp#2 -- true").trim().to_owned();
    rig.out("queue add laptop life#1 -- true");
    rig.out("queue take laptop");
    let (code, _, err) = rig.run(&format!("queue msg {id} done token ghp_x"));
    assert_eq!((code, err.as_str()), (1, "NorthThreads: betterleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored\n"));
    assert!(rig.outside.calls.borrow().is_empty());
    rig.no_native = Some("THREADS_NO_NATIVE is set (agy)");
    assert!(rig.out("queue").lines().nth(2).unwrap().starts_with("2   laptop     native  queued"), "{}", rig.out("queue"));
    assert_eq!(rig.run("queue add laptop life#2 -- true"), (1, String::new(), "NorthThreads: native items are not readable here: THREADS_NO_NATIVE is set (agy)\n".into()));
}
