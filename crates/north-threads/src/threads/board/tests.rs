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
CREATE TABLE items(ns TEXT NOT NULL, number INTEGER NOT NULL CHECK(number > 0), title TEXT NOT NULL CHECK(length(title) > 0), body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'open' CHECK(state IN ('open','closed')), created TEXT NOT NULL, closed TEXT, due TEXT, PRIMARY KEY(ns, number)) STRICT;
CREATE TABLE item_needs(item TEXT NOT NULL, needs TEXT NOT NULL, PRIMARY KEY(item, needs)) STRICT;
CREATE TABLE runs(agent TEXT PRIMARY KEY, item TEXT, follows TEXT, tier TEXT NOT NULL, ended TEXT NOT NULL, minutes INTEGER NOT NULL, eta_min INTEGER, outcome TEXT NOT NULL, report TEXT NOT NULL DEFAULT '', note TEXT);";
const NOW: &str = "2026-10-28T09:00:00Z";
const HIDDEN: &str = "CLAUDE_CODE_REMOTE is set (cloud worker)";

fn page(nodes: &[&str], next: Option<&str>) -> String {
    let info = next.map_or_else(|| r#"{"hasNextPage":false,"endCursor":null}"#.to_owned(), |cursor| format!(r#"{{"hasNextPage":true,"endCursor":"{cursor}"}}"#));
    format!(r#"{{"data":{{"repository":{{"issues":{{"pageInfo":{info},"nodes":[{}]}}}}}}}}"#, nodes.join(","))
}

fn node(number: u64, blockers: &[(u64, &str)]) -> String {
    let blockers: Vec<String> = blockers.iter().map(|(number, state)| format!(r#"{{"number":{number},"state":"{state}","repository":{{"nameWithOwner":"tompassarelli/makgora"}}}}"#)).collect();
    format!(r#"{{"number":{number},"title":"T{number}","blockedBy":{{"nodes":[{}]}}}}"#, blockers.join(","))
}

/// makgora: #1 blocked by open #2, #2 open, #3 open, #4 blocked by closed #5; no other repo has open issues.
fn makgora() -> String {
    page(&[&node(1, &[(2, "OPEN")]), &node(2, &[]), &node(3, &[]), &node(4, &[(5, "CLOSED")])], None)
}

struct Ledger {
    _directory: tempfile::TempDir,
    path: PathBuf,
    outside: Recorded,
    no_native: Option<&'static str>,
    replica: bool,
}

fn ledger() -> Ledger {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA).unwrap();
    let outside = Recorded { reply: Box::leak(page(&[], None).into_boxed_str()), answers: vec![("name=makgora", makgora())], ..Recorded::default() };
    Ledger { _directory: directory, path, outside, no_native: None, replica: false }
}

impl Ledger {
    fn exec(&self, sql: &str) {
        Connection::open(&self.path).unwrap().execute_batch(sql).unwrap();
    }

    fn sql(&self, query: &str) -> String {
        Connection::open(&self.path).unwrap().query_row(query, [], |row| Ok(match row.get_ref(0)? {
            ValueRef::Null => String::new(),
            ValueRef::Integer(number) => number.to_string(),
            ValueRef::Real(number) => number.to_string(),
            other => String::from_utf8_lossy(other.as_bytes().unwrap()).into_owned(),
        })).unwrap()
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: self.no_native, outside: &self.outside, now: Timestamp::parse(NOW).unwrap(), replica: self.replica };
        let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &context, &mut &b""[..], &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn out(&self, argv: &[&str]) -> String {
        let (code, out, err) = self.run(argv);
        assert_eq!((code, err.as_str()), (0, ""), "{argv:?}");
        out
    }

    fn ready(&self) -> Vec<String> {
        self.out(&["ready"]).lines().map(|line| line.split('\t').next().unwrap().to_owned()).collect()
    }

}

#[test]
fn a_replica_ledger_answers_from_its_cache_and_is_never_written() {
    let mut rig = ledger();
    rig.exec("INSERT INTO gh_cache VALUES ('wisp', 1.0, '{}')");
    rig.replica = true;
    assert_eq!(rig.ready(), ["makgora#2", "makgora#3", "makgora#4"]);
    assert_eq!(rig.sql("SELECT group_concat(repo || ' ' || fetched) FROM gh_cache"), "wisp 1.0");
}

#[test]
fn a_caller_that_may_not_read_native_items_is_shown_none() {
    let mut rig = ledger();
    rig.exec("INSERT INTO items(ns, number, title, created) VALUES ('life', 1, 'p a', 'x'); INSERT INTO item_needs VALUES ('makgora#3', 'life#1'); INSERT INTO claims VALUES ('life#1', 'alice', '2026-10-28T08:00:00Z', NULL)");
    rig.no_native = Some(HIDDEN);
    assert_eq!(rig.ready(), ["makgora#2", "makgora#4"]);
    assert_eq!(rig.run(&["show", "life#1"]), (1, String::new(), format!("NorthThreads: native items are not readable here: {HIDDEN}\n")));
    assert_eq!(rig.out(&["show", "makgora#3"]), "makgora#3  T3\n  state: open\n  blocked by: a native item (open)\n");
    assert_eq!(rig.ready(), ["makgora#2", "makgora#4"]);
    rig.exec("DELETE FROM claims");
    assert_eq!(rig.ready(), ["makgora#2", "makgora#4"]);
    rig.no_native = None;
    assert_eq!(rig.ready(), ["makgora#2", "makgora#4", "life#1"]);
}

