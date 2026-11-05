use std::path::PathBuf;

use rusqlite::Connection;
use rusqlite::types::ValueRef;

use super::run;
use crate::foundation::clock::Timestamp;
use crate::threads::fake::Recorded;
use crate::threads::verb::Context;

const SCHEMA: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL, actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE TABLE claims(item TEXT PRIMARY KEY CHECK(item GLOB '*#[0-9]*'), owner TEXT NOT NULL CHECK(length(owner) > 0), since TEXT NOT NULL, eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)) STRICT;
CREATE TABLE gh_cache(repo TEXT PRIMARY KEY, fetched REAL NOT NULL, issues TEXT NOT NULL CHECK(json_valid(issues))) STRICT;
CREATE TABLE blocks(item TEXT PRIMARY KEY, reason TEXT NOT NULL, since TEXT NOT NULL, by TEXT NOT NULL) STRICT;
CREATE TABLE items(ns TEXT NOT NULL, number INTEGER NOT NULL CHECK(number > 0), title TEXT NOT NULL CHECK(length(title) > 0), body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'open' CHECK(state IN ('open','closed')), created TEXT NOT NULL, closed TEXT, due TEXT, PRIMARY KEY(ns, number)) STRICT;
CREATE TABLE item_needs(item TEXT NOT NULL, needs TEXT NOT NULL, PRIMARY KEY(item, needs)) STRICT;";
const NOW: &str = "2026-10-28T09:00:00Z";

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
    Ledger { _directory: directory, path, outside: Recorded { reply: "4242\n", ..Recorded::default() }, no_native: None }
}

impl Ledger {
    fn sql(&self, query: &str) -> String {
        Connection::open(&self.path).unwrap().query_row(query, [], |row| Ok(match row.get_ref(0)? {
            ValueRef::Null => String::new(),
            ValueRef::Integer(number) => number.to_string(),
            other => String::from_utf8_lossy(other.as_bytes().unwrap()).into_owned(),
        })).unwrap()
    }

    fn feed(&self, argv: &[&str], stdin: &str) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: self.no_native, outside: &self.outside, now: Timestamp::parse(NOW).unwrap(), replica: false };
        let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &context, &mut stdin.as_bytes(), &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        self.feed(argv, "")
    }

    fn out(&self, argv: &[&str]) -> String {
        let (code, out, err) = self.run(argv);
        assert_eq!((code, err.as_str()), (0, ""), "{argv:?}");
        out
    }

}

#[test]
fn note_and_new_refuse_text_the_secret_scan_flags_and_store_nothing() {
    let mut rig = ledger();
    rig.out(&["new", "life", "p a"]);
    rig.outside.secret = Some("AKIA");
    let refused = (1, String::new(), "NorthThreads: betterleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored\n".to_owned());
    assert_eq!(rig.run(&["note", "life#1", "creds", "AKIAX"]), refused);
    assert_eq!(rig.run(&["new", "life", "fake AKIAX"]), refused);
    assert_eq!(rig.feed(&["new", "life", "fine", "--body", "-"], "AKIAX"), refused);
    assert_eq!(rig.sql("SELECT count(*) || '/' || (SELECT count(*) FROM items) FROM events"), "1/1");
}

