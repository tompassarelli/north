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
    let blockers: Vec<String> = blockers.iter().map(|(number, state)| format!(r#"{{"number":{number},"state":"{state}","repository":{{"nameWithOwner":"tompassarelli/smashcraft"}}}}"#)).collect();
    format!(r#"{{"number":{number},"title":"T{number}","blockedBy":{{"nodes":[{}]}}}}"#, blockers.join(","))
}

/// smashcraft: #1 blocked by open #2, #2 open, #3 open, #4 blocked by closed #5; no other repo has open issues.
fn smashcraft() -> String {
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
    let outside = Recorded { reply: Box::leak(page(&[], None).into_boxed_str()), answers: vec![("name=smashcraft", smashcraft())], ..Recorded::default() };
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

    fn calls(&self) -> Vec<String> {
        self.outside.calls.borrow().iter().map(|call| call.join(" ")).collect()
    }
}

#[test]
fn ready_lists_open_issues_minus_open_blockers_live_claims_and_claims_of_live_owners() {
    let mut rig = ledger();
    rig.outside.alive = vec!["carol"];
    rig.exec("INSERT INTO claims VALUES ('smashcraft#3', 'bob', '2026-10-28T08:00:00Z', 30), ('smashcraft#2', 'carol', '2026-10-20T08:00:00Z', 30), ('smashcraft#4', 'dave', '2026-10-20T08:00:00Z', 30)");
    assert_eq!(rig.out(&["ready", "--by", "x"]), "smashcraft#4\tT4\n");
    rig.outside.alive.clear();
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#4"]);
    assert_eq!(rig.run(&["ready", "smashcraft#1"]).0, 2);
}

#[test]
fn open_issues_are_read_once_a_minute_through_a_cache_bin_threads_reads_too() {
    let mut rig = ledger();
    rig.outside.answers.insert(0, ("cursor=c1", page(&[&node(7, &[])], None)));
    rig.outside.answers.insert(1, ("name=wisp", page(&[&node(6, &[])], Some("c1"))));
    rig.ready();
    let queries: Vec<String> = rig.calls().iter().map(|call| call.rsplit(" -F ").take(2).collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(queries.len(), 9);
    assert!(queries.contains(&"cursor=c1 name=wisp".to_owned()), "{queries:?}");
    assert!(rig.calls()[0].starts_with("api graphql -f query=\nquery($owner: String!, $name: String!, $cursor: String) {\n"));
    assert_eq!(rig.sql("SELECT issues FROM gh_cache WHERE repo = 'wisp'"), r#"{"6": {"title": "T6", "blockers": []}, "7": {"title": "T7", "blockers": []}}"#);
    assert_eq!(
        rig.sql("SELECT issues FROM gh_cache WHERE repo = 'smashcraft'"),
        r#"{"1": {"title": "T1", "blockers": [{"ref": "smashcraft#2", "state": "OPEN"}]}, "2": {"title": "T2", "blockers": []}, "3": {"title": "T3", "blockers": []}, "4": {"title": "T4", "blockers": [{"ref": "smashcraft#5", "state": "CLOSED"}]}}"#
    );
    assert_eq!(rig.sql("SELECT fetched FROM gh_cache WHERE repo = 'wisp'"), Timestamp::parse(NOW).unwrap().unix_seconds_f64().to_string());
    rig.outside.calls.borrow_mut().clear();
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#3", "smashcraft#4", "wisp#6", "wisp#7"]);
    assert!(rig.calls().is_empty());
    rig.exec("UPDATE gh_cache SET fetched = fetched - 60");
    rig.ready();
    assert_eq!(rig.calls().len(), 9);
}

#[test]
fn a_replica_ledger_answers_from_its_cache_and_is_never_written() {
    let mut rig = ledger();
    rig.exec("INSERT INTO gh_cache VALUES ('wisp', 1.0, '{}')");
    rig.replica = true;
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#3", "smashcraft#4"]);
    assert_eq!(rig.sql("SELECT group_concat(repo || ' ' || fetched) FROM gh_cache"), "wisp 1.0");
}

#[test]
fn an_unreadable_repository_is_skipped_with_a_line_and_the_rest_are_listed() {
    let mut rig = ledger();
    rig.outside.refuse = Some("name=wisp");
    let (code, out, err) = rig.run(&["ready"]);
    assert_eq!((code, out.as_str()), (0, "smashcraft#2\tT2\nsmashcraft#3\tT3\nsmashcraft#4\tT4\n"));
    assert_eq!(err, "NorthThreads: skipped wisp: gh api graphql: refused\n");
}

#[test]
fn native_items_are_blocked_and_freed_locally_and_never_reach_github() {
    let rig = ledger();
    rig.exec("INSERT INTO items(ns, number, title, body, created, due) VALUES ('life', 1, 'p a', 'line one\nline two', 'x', NULL), ('life', 2, 'p b', '', 'x', '2026-12-01');
        INSERT INTO item_needs VALUES ('life#1', 'life#2'), ('smashcraft#4', 'life#2'), ('life#2', 'smashcraft#1');
        INSERT INTO events(ts, item, kind, actor, data) VALUES ('2026-10-28T08:00:00Z', 'life#1', 'note', 'a', '{\"text\": \"called\"}'), ('2026-10-28T08:01:00Z', 'life#1', 'note', 'a', '{\"block\": \"x\"}')");
    assert_eq!(rig.out(&["show", "life#1"]), "life#1  p a\n  state: open\n  blocked by: life#2 (open)\n  line one\n  line two\n  note 2026-10-28T08:00:00Z  called\n");
    assert_eq!(rig.out(&["show", "life#2"]), "life#2  p b\n  state: open\n  blocked by: smashcraft#1 (open)\n  due: 2026-12-01\n");
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#3"]);
    rig.exec("UPDATE items SET state = 'closed' WHERE number = 2");
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#3", "smashcraft#4", "life#1"]);
    assert_eq!(rig.out(&["show", "life#2"]).lines().nth(1), Some("  state: closed"));
    assert!(rig.calls().iter().all(|call| !call.contains("life")), "{:?}", rig.calls());
    assert_eq!(rig.run(&["show", "life#9"]), (1, String::new(), "NorthThreads: no item life#9\n".into()));
}

#[test]
fn a_caller_that_may_not_read_native_items_is_shown_none() {
    let mut rig = ledger();
    rig.exec("INSERT INTO items(ns, number, title, created) VALUES ('life', 1, 'p a', 'x'); INSERT INTO item_needs VALUES ('smashcraft#3', 'life#1'); INSERT INTO claims VALUES ('life#1', 'alice', '2026-10-28T08:00:00Z', NULL)");
    rig.no_native = Some(HIDDEN);
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#4"]);
    assert_eq!(rig.run(&["show", "life#1"]), (1, String::new(), format!("NorthThreads: native items are not readable here: {HIDDEN}\n")));
    assert_eq!(rig.out(&["show", "smashcraft#3"]), "smashcraft#3  T3\n  state: open\n  blocked by: a native item (open)\n");
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#4"]);
    rig.exec("DELETE FROM claims");
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#4"]);
    rig.no_native = None;
    assert_eq!(rig.ready(), ["smashcraft#2", "smashcraft#4", "life#1"]);
}

#[test]
fn show_prints_an_issue_with_its_owner_handoffs_runs_and_archives() {
    let mut rig = ledger();
    rig.outside.archived = "  archived: archive/x\n";
    rig.ready();
    rig.outside.calls.borrow_mut().clear();
    rig.exec("INSERT INTO claims VALUES ('smashcraft#3', 'bob', '2026-10-28T08:29:30Z', 45);
        INSERT INTO events(ts, item, kind, actor, data) VALUES ('2026-10-28T08:00:00Z', 'smashcraft#3', 'claim', 'alice', '{}'), ('2026-10-28T08:01:00Z', 'smashcraft#3', 'note', 'alice', '{\"text\": \"x\"}'), ('2026-10-28T08:02:00Z', 'smashcraft#3', 'handoff', 'alice', '{\"from\": \"alice\", \"to\": \"bob\"}');
        INSERT INTO runs VALUES ('a222', 'smashcraft#3', 'a111', 'xhigh', '2026-10-08T02:00:00Z', 50, 45, 'done', 'Done: shipped', NULL), ('a111', 'smashcraft#3', NULL, 'high', '2026-10-08T01:00:00Z', 30, NULL, 'handoff', '', '~/h/a111.md')");
    assert_eq!(
        rig.out(&["show", "tompassarelli/smashcraft#3"]),
        "smashcraft#3  T3\n  state: open\n  owner: bob for 30m of 45m ETA\n  owners:\n    2026-10-28T08:00:00Z  claim    alice\n    2026-10-28T08:02:00Z  handoff  alice -> bob\n  runs: 2, 80m in all\n    2026-10-08T01:00  a111 high  30m  handoff:   note ~/h/a111.md\n    2026-10-08T02:00  a222 xhigh follows a111  50m/45m ETA  done: Done: shipped\n  archived: archive/x\n"
    );
    assert!(rig.calls().is_empty(), "the cached open issues answer it: {:?}", rig.calls());
}

#[test]
fn an_issue_that_is_not_open_is_read_alone_and_one_that_does_not_exist_is_refused() {
    let mut rig = ledger();
    rig.outside.answers.insert(0, ("number=9", r#"{"data":{"repository":{"issue":{"number":9,"title":"Gone","state":"CLOSED","blockedBy":{"nodes":[]}}}}}"#.into()));
    rig.outside.answers.insert(0, ("number=8", r#"{"data":{"repository":{"issue":null}}}"#.into()));
    rig.exec("INSERT INTO item_needs VALUES ('smashcraft#9', 'smashcraft#1')");
    assert_eq!(rig.out(&["show", "smashcraft#9"]), "smashcraft#9  Gone\n  state: closed\n  blocked by: smashcraft#1 (open)\n");
    assert_eq!(rig.run(&["show", "smashcraft#8"]), (1, String::new(), "NorthThreads: no issue smashcraft#8\n".into()));
    assert!(rig.calls()[0].starts_with("api graphql -f query=\nquery($owner: String!, $name: String!, $number: Int!) {\n") && rig.calls()[0].ends_with("-F owner=tompassarelli -F name=smashcraft -F number=9"));
}
