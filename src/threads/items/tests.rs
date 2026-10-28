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

    fn events(&self) -> String {
        self.sql("SELECT group_concat(item || ' ' || kind || ' ' || actor || ' ' || data, char(10)) FROM (SELECT * FROM events ORDER BY seq)")
    }
}

#[test]
fn release_to_hands_the_issue_over_with_a_handoff_event_and_release_lets_it_go_naming_its_owner() {
    let rig = ledger();
    Connection::open(&rig.path).unwrap().execute("INSERT INTO claims VALUES ('smashcraft#3', 'alice', '2026-10-28T08:00:00Z', 30)", []).unwrap();
    assert_eq!(rig.out(&["release", "smashcraft#3", "--by", "alice", "--to", "bob"]), "smashcraft#3 handed from alice to bob\n");
    assert_eq!(rig.sql("SELECT owner || ' ' || since || ' ' || eta_min FROM claims"), format!("bob {NOW} 30"));
    assert_eq!(rig.out(&["release", "smashcraft#3", "--to", ""]), "smashcraft#3 released by bob\n");
    assert_eq!(rig.run(&["release", "smashcraft#3"]), (1, String::new(), "NorthThreads: smashcraft#3 has no owner\n".into()));
    assert_eq!(rig.events(), "smashcraft#3 handoff alice {\"from\": \"alice\", \"to\": \"bob\"}\nsmashcraft#3 release tester {\"owner\": \"bob\"}");
}

#[test]
fn native_items_are_numbered_per_namespace_and_their_needs_notes_and_state_stay_in_the_ledger() {
    let rig = ledger();
    assert_eq!(rig.out(&["new", "life", "p a"]), "life#1\n");
    assert_eq!(rig.feed(&["new", "life", "p b", "--due", "2026-12-01", "--body", "-"], "  the body\n\n").1, "life#2\n");
    assert_eq!(rig.out(&["new", "ops", "o"]), "ops#1\n");
    assert_eq!(rig.sql("SELECT group_concat(ns || number || '|' || title || '|' || body || '|' || ifnull(due, '-'), ';') FROM items"), "life1|p a||-;life2|p b|the body|2026-12-01;ops1|o||-");
    assert_eq!(rig.out(&["need", "life#1", "life#2"]), "life#1 is blocked by life#2 locally\n");
    assert_eq!(rig.out(&["need", "smashcraft#4", "life#2"]), "smashcraft#4 is blocked by life#2 locally\n");
    assert_eq!(rig.out(&["unneed", "life#1", "life#2"]), "life#1 is no longer blocked by life#2\n");
    assert_eq!(rig.sql("SELECT group_concat(item || '>' || needs) FROM item_needs"), "smashcraft#4>life#2");
    assert_eq!(rig.out(&["note", "life#1", "--", "called", "-1", "day"]), "noted on life#1\n");
    assert_eq!(rig.out(&["close", "life#2"]), "life#2 closed\n");
    assert_eq!(rig.sql("SELECT state || ' ' || closed FROM items WHERE number = 2"), format!("closed {NOW}"));
    assert_eq!(rig.out(&["reopen", "life#2"]), "life#2 open\n");
    assert_eq!(rig.sql("SELECT state || ' ' || ifnull(closed, '-') FROM items WHERE number = 2"), "open -");
    assert!(rig.outside.calls.borrow().is_empty());
    assert_eq!(
        rig.events(),
        "life#1 capture tester {}\nlife#2 capture tester {}\nops#1 capture tester {}\nlife#1 need tester {\"needs\": \"life#2\"}\nsmashcraft#4 need tester {\"needs\": \"life#2\"}\nlife#1 unneed tester {\"needs\": \"life#2\"}\nlife#1 note tester {\"text\": \"called -1 day\"}\nlife#2 close tester {}\nlife#2 reopen tester {}"
    );
}

#[test]
fn note_and_new_refuse_text_the_secret_scan_flags_and_store_nothing() {
    let mut rig = ledger();
    rig.out(&["new", "life", "p a"]);
    rig.outside.secret = Some("AKIA");
    let refused = (1, String::new(), "NorthThreads: gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored\n".to_owned());
    assert_eq!(rig.run(&["note", "life#1", "creds", "AKIAX"]), refused);
    assert_eq!(rig.run(&["new", "life", "fake AKIAX"]), refused);
    assert_eq!(rig.feed(&["new", "life", "fine", "--body", "-"], "AKIAX"), refused);
    assert_eq!(rig.sql("SELECT count(*) || '/' || (SELECT count(*) FROM items) FROM events"), "1/1");
}

#[test]
fn notes_and_state_changes_go_only_to_native_items_that_exist_and_hidden_callers_cannot_reach_them() {
    let mut rig = ledger();
    assert_eq!(rig.run(&["note", "wisp#2", "hi"]), (1, String::new(), "NorthThreads: wisp#2 is a GitHub issue; only life, accounts, travel, ops items are native\n".into()));
    assert_eq!(rig.run(&["close", "life#9"]), (1, String::new(), "NorthThreads: no item life#9\n".into()));
    assert_eq!(rig.run(&["new", "wisp", "x"]), (1, String::new(), "NorthThreads: wisp is not a native namespace (life, accounts, travel, ops)\n".into()));
    assert_eq!(rig.run(&["note", "life#1"]).0, 2);
    rig.no_native = Some("CLAUDE_CODE_REMOTE is set (cloud worker)");
    assert_eq!(rig.run(&["need", "wisp#2", "life#1"]), (1, String::new(), "NorthThreads: native items are not readable here: CLAUDE_CODE_REMOTE is set (cloud worker)\n".into()));
}

#[test]
fn a_github_need_and_unneed_go_through_the_dependency_api_and_drop_the_cached_open_issues() {
    let rig = ledger();
    Connection::open(&rig.path).unwrap().execute("INSERT INTO gh_cache VALUES ('firn', 1.0, '{}'), ('wisp', 1.0, '{}')", []).unwrap();
    assert_eq!(rig.out(&["need", "nixos-config#3", "wisp#7"]), "nixos-config#3 is blocked by wisp#7 on GitHub\n");
    assert_eq!(rig.out(&["unneed", "firn#3", "wisp#7"]), "firn#3 is no longer blocked by wisp#7\n");
    let calls: Vec<String> = rig.outside.calls.borrow().iter().map(|call| call.join(" ")).collect();
    assert_eq!(calls, [
        "api repos/tompassarelli/wisp/issues/7 --jq .id",
        "api -X POST repos/tompassarelli/firn/issues/3/dependencies/blocked_by -F issue_id=4242",
        "api repos/tompassarelli/wisp/issues/7 --jq .id",
        "api -X DELETE repos/tompassarelli/firn/issues/3/dependencies/blocked_by/4242",
    ]);
    assert_eq!(rig.sql("SELECT group_concat(repo) FROM gh_cache"), "wisp");
    assert_eq!(rig.events(), "firn#3 need tester {\"needs\": \"wisp#7\"}\nfirn#3 unneed tester {\"needs\": \"wisp#7\"}");
}

#[test]
fn block_records_why_an_issue_cannot_be_staffed_and_clear_removes_it() {
    let rig = ledger();
    assert_eq!(rig.run(&["block", "wisp#20"]), (1, String::new(), "NorthThreads: block needs a reason\n".into()));
    assert_eq!(rig.out(&["block", "wisp#20", "waits", "on", "smashcraft#13", "--by", "lead"]), "wisp#20 blocked: waits on smashcraft#13\n");
    assert_eq!(rig.out(&["block", "wisp#20", "waits on Tom"]), "wisp#20 blocked: waits on Tom\n");
    assert_eq!(rig.sql("SELECT reason || ' ' || since || ' ' || by FROM blocks"), format!("waits on Tom {NOW} tester"));
    assert_eq!(rig.out(&["block", "wisp#20", "--clear"]), "wisp#20 unblocked\n");
    assert_eq!(rig.sql("SELECT count(*) FROM blocks"), "0");
    assert_eq!(rig.events(), "wisp#20 note lead {\"block\": \"waits on smashcraft#13\"}\nwisp#20 note tester {\"block\": \"waits on Tom\"}\nwisp#20 note tester {\"unblock\": true}");
}
