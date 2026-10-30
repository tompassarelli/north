use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rusqlite::{Connection, params};
use serde_json::Value;

use super::issues::{self, Issues};
use super::verbs::{self, Context};
use super::Issue;
use crate::foundation::clock::Timestamp;
use crate::ops::dispatch::claim_rows;

const SCHEMA: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL, actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE TABLE claims(item TEXT PRIMARY KEY, owner TEXT NOT NULL, since TEXT NOT NULL, eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)) STRICT;
CREATE TABLE items(ns TEXT NOT NULL, number INTEGER NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'open', created TEXT NOT NULL, closed TEXT, due TEXT, PRIMARY KEY(ns, number)) STRICT;";
const NOW: &str = "2026-10-27T12:00:00Z";

struct Seen(BTreeMap<(String, u64), Issue>);

impl Issues for Seen {
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue> {
        items.iter().filter_map(|item| Some((item.clone(), *self.0.get(item)?))).collect()
    }
}

struct Ledger {
    _directory: tempfile::TempDir,
    path: PathBuf,
    issues: Seen,
    no_native: Option<&'static str>,
    alive: Option<BTreeSet<String>>,
}

fn at(text: &str) -> i64 {
    Timestamp::parse(text).unwrap().unix_seconds()
}

fn ledger(issues: &[(u64, Issue)]) -> Ledger {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA).unwrap();
    let issues = Seen(issues.iter().map(|(number, issue)| (("smashcraft".to_owned(), *number), *issue)).collect());
    Ledger { _directory: directory, path, issues, no_native: None, alive: None }
}

impl Ledger {
    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    fn claimed(&self, item: &str, owner: &str, since: &str, eta_min: Option<i64>) {
        self.db().execute("INSERT INTO claims VALUES (?1, ?2, ?3, ?4)", params![item, owner, since, eta_min]).unwrap();
        self.db().execute("INSERT INTO events(ts, item, kind, actor) VALUES (?1, ?2, 'claim', ?3)", params![since, item, owner]).unwrap();
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: self.no_native, issues: &self.issues, alive: self.alive.clone(), now: at(NOW) };
        let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = verbs::run(&argv, &context, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    /// The issues dispatch's survey holds from this ledger's `threads list --json`.
    fn dispatch_holds(&self) -> Vec<String> {
        let (code, listing, _) = self.run(&["list", "--json"]);
        assert_eq!(code, 0);
        claim_rows(&serde_json::from_str::<Value>(&listing).unwrap(), "nexus", &Default::default()).into_iter().map(|(item, _)| item.to_string()).collect()
    }

    fn listed(&self) -> Vec<String> {
        self.run(&["list"]).1.lines().skip(1).map(|line| line.split_whitespace().next().unwrap().to_owned()).collect()
    }

    fn events(&self, item: &str) -> Vec<(String, String, Value)> {
        let db = self.db();
        let mut statement = db.prepare("SELECT kind, actor, data FROM events WHERE item = ?1 ORDER BY seq").unwrap();
        statement
            .query_map([item], |row| Ok((row.get(0)?, row.get(1)?, serde_json::from_str(&row.get::<_, String>(2)?).unwrap())))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn owner(&self, item: &str) -> String {
        self.db().query_row("SELECT owner FROM claims WHERE item = ?1", [item], |row| row.get(0)).unwrap()
    }
}

#[test]
fn a_claim_made_before_its_issue_opened_is_ignored_by_claim_list_and_dispatch() {
    let ledger = ledger(&[(523, Issue::Open { opened: at("2026-10-26T08:00:00Z") })]);
    ledger.claimed("smashcraft#523", "art-a8f12c1a", "2026-10-12T09:00:00Z", Some(60));
    assert!(ledger.listed().is_empty());
    assert!(ledger.dispatch_holds().is_empty());
    let (code, out, err) = ledger.run(&["claim", "smashcraft#523", "--by", "sc-523-w", "--eta", "45"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "sc-523-w owns smashcraft#523; the claim by art-a8f12c1a was void because it was made before its issue was opened\n");
    assert_eq!(ledger.owner("smashcraft#523"), "sc-523-w");
    assert_eq!(ledger.dispatch_holds(), ["smashcraft#523"]);
}

#[test]
fn a_closed_issues_claim_is_ignored_by_claim_list_and_dispatch() {
    let ledger = ledger(&[(453, Issue::Closed), (454, Issue::Open { opened: at("2026-10-01T00:00:00Z") })]);
    ledger.claimed("smashcraft#453", "codex:/root", "2026-10-27T11:00:00Z", Some(180));
    ledger.claimed("smashcraft#454", "codex:/root", "2026-10-27T11:00:00Z", Some(180));
    assert_eq!(ledger.listed(), ["smashcraft#454"]);
    assert_eq!(ledger.dispatch_holds(), ["smashcraft#454"]);
    let (code, out, _) = ledger.run(&["claim", "smashcraft#453", "--by", "sc-453-w"]);
    assert_eq!((code, out.as_str()), (0, "sc-453-w owns smashcraft#453; the claim by codex:/root was void because its issue is closed\n"));
}

#[test]
fn a_claim_a_day_past_its_eta_with_no_newer_owner_event_is_taken_over_with_a_takeover_event() {
    let opened = Issue::Open { opened: at("2026-10-20T00:00:00Z") };
    let ledger = ledger(&[(51, opened), (52, opened)]);
    ledger.claimed("smashcraft#51", "ad4af7badd42fb5f7", "2026-10-25T10:00:00Z", Some(60));
    ledger.claimed("smashcraft#52", "w52", "2026-10-25T10:00:00Z", Some(60));
    ledger.db().execute("INSERT INTO events(ts, item, kind, actor) VALUES ('2026-10-26T13:00:00Z', 'smashcraft#52', 'note', 'w52')", []).unwrap();
    let (_, list, _) = ledger.run(&["list"]);
    assert!(list.lines().any(|line| line.starts_with("smashcraft#51") && line.ends_with("STALE")), "{list}");
    assert!(list.lines().any(|line| line.starts_with("smashcraft#52") && line.ends_with("OVERDUE")), "{list}");
    assert_eq!(ledger.dispatch_holds(), ["smashcraft#52"]);
    let (code, out, _) = ledger.run(&["claim", "smashcraft#51", "--by", "sc-51-w", "--eta", "30"]);
    assert_eq!((code, out.as_str()), (0, "sc-51-w owns smashcraft#51, taken over from ad4af7badd42fb5f7: stale, no owner event since 2026-10-25T11:00:00Z\n"));
    let (kind, actor, data) = ledger.events("smashcraft#51").pop().unwrap();
    assert_eq!((kind.as_str(), actor.as_str()), ("handoff", "sc-51-w"));
    assert_eq!(data, serde_json::json!({"from": "ad4af7badd42fb5f7", "to": "sc-51-w", "takeover": "stale: no owner event since 2026-10-25T11:00:00Z"}));
    let (code, _, err) = ledger.run(&["claim", "smashcraft#52", "--by", "sc-52-w"]);
    assert_eq!((code, err.as_str()), (1, "NorthThreads: smashcraft#52 is already owned by w52\n"));
}

#[test]
fn a_live_claim_still_refuses_and_dispatch_holds_it_even_when_github_is_unread() {
    let ledger = ledger(&[(492, Issue::Open { opened: at("2026-10-27T03:00:00Z") })]);
    ledger.claimed("smashcraft#492", "tom", "2026-10-27T04:18:00Z", None);
    ledger.claimed("smashcraft#600", "w600", "2026-10-27T10:00:00Z", Some(90));
    let (code, out, err) = ledger.run(&["claim", "tompassarelli/smashcraft#492", "--by", "sc-492-w"]);
    assert_eq!((code, out.as_str(), err.as_str()), (1, "", "NorthThreads: smashcraft#492 is already owned by tom\n"));
    assert_eq!(ledger.run(&["claim", "smashcraft#600"]).0, 1);
    assert_eq!(ledger.owner("smashcraft#492"), "tom");
    assert_eq!(ledger.dispatch_holds(), ["smashcraft#492", "smashcraft#600"]);
    assert_eq!(ledger.run(&["claim", "smashcraft#492", "--by", "tom", "--eta", "90"]).1, "tom owns smashcraft#492\n");
}

#[test]
fn github_answers_a_missing_issue_beside_the_found_ones_and_an_unread_reply_voids_nothing() {
    let items: BTreeSet<(String, u64)> = [("smashcraft".to_owned(), 453), ("smashcraft".to_owned(), 4824), ("wisp".to_owned(), 7)].into();
    assert_eq!(
        issues::query(&items),
        "query { r0: repository(owner: \"tompassarelli\", name: \"smashcraft\") { i453: issue(number: 453) { state createdAt } i4824: issue(number: 4824) { state createdAt } } r1: repository(owner: \"tompassarelli\", name: \"wisp\") { i7: issue(number: 7) { state createdAt } } }"
    );
    let reply = serde_json::json!({"data": {"r0": {"i453": {"state": "CLOSED", "createdAt": "2026-10-26T08:09:49Z"}, "i4824": null}, "r1": {"i7": {"state": "OPEN", "createdAt": "2026-10-01T00:00:00Z"}}}});
    let found = issues::answers(&items, &reply);
    assert_eq!(found.values().copied().collect::<Vec<_>>(), [Issue::Closed, Issue::Missing, Issue::Open { opened: at("2026-10-01T00:00:00Z") }]);
    assert!(issues::answers(&items, &Value::Null).values().all(|issue| *issue == Issue::Unread));
}

#[test]
fn a_caller_that_may_not_read_native_items_lists_no_native_claim() {
    let mut ledger = ledger(&[(3, Issue::Open { opened: at("2026-10-01T00:00:00Z") })]);
    ledger.db().execute("INSERT INTO items(ns, number, title, created) VALUES ('life', 1, 'p a', 'x')", []).unwrap();
    ledger.claimed("life#1", "alice", "2026-10-27T11:00:00Z", None);
    ledger.claimed("smashcraft#3", "bob", "2026-10-27T11:00:00Z", None);
    assert_eq!(ledger.listed(), ["life#1", "smashcraft#3"]);
    ledger.no_native = Some("THREADS_NO_NATIVE is set (agy)");
    assert_eq!(ledger.listed(), ["smashcraft#3"]);
    assert!(!ledger.run(&["list", "--json"]).1.contains("life#"));
    assert_eq!(ledger.run(&["claim", "life#1"]).2, "NorthThreads: native items are not readable here: THREADS_NO_NATIVE is set (agy)\n");
}

#[test]
fn a_claim_whose_owner_is_not_a_live_fleet_session_is_void_and_the_next_claimer_takes_it() {
    let opened = Issue::Open { opened: at("2026-10-01T00:00:00Z") };
    let mut ledger = ledger(&[(309, opened), (310, opened)]);
    ledger.claimed("smashcraft#309", "c00dae06", "2026-10-27T10:00:00Z", Some(40));
    ledger.claimed("smashcraft#310", "w310", "2026-10-27T11:30:00Z", Some(120));
    ledger.alive = Some(BTreeSet::from(["w310".to_owned()]));
    assert_eq!(ledger.listed(), ["smashcraft#310"]);
    assert_eq!(ledger.dispatch_holds(), ["smashcraft#310"]);
    let (code, out, err) = ledger.run(&["claim", "smashcraft#309", "--by", "sc-309-w", "--eta", "30"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "sc-309-w owns smashcraft#309; the claim by c00dae06 was void because its owner is not a live fleet session\n");
    assert_eq!(ledger.owner("smashcraft#309"), "sc-309-w");
    let (code, _, err) = ledger.run(&["claim", "smashcraft#310", "--by", "sc-310-w"]);
    assert_eq!((code, err.as_str()), (1, "NorthThreads: smashcraft#310 is already owned by w310\n"));
}

#[test]
fn an_unread_fleet_voids_no_claim() {
    let ledger = ledger(&[(309, Issue::Open { opened: at("2026-10-01T00:00:00Z") })]);
    ledger.claimed("smashcraft#309", "c00dae06", "2026-10-27T11:00:00Z", Some(120));
    assert_eq!(ledger.listed(), ["smashcraft#309"]);
    assert_eq!(ledger.run(&["claim", "smashcraft#309", "--by", "sc-309-w"]).0, 1);
}

#[test]
fn a_claim_past_twice_its_eta_is_taken_unless_its_owner_recorded_an_event_since_the_eta() {
    let opened = Issue::Open { opened: at("2026-10-01T00:00:00Z") };
    let ledger = ledger(&[(61, opened), (62, opened)]);
    ledger.claimed("smashcraft#61", "w61", "2026-10-27T10:00:00Z", Some(40));
    ledger.claimed("smashcraft#62", "w62", "2026-10-27T10:00:00Z", Some(40));
    ledger.db().execute("INSERT INTO events(ts, item, kind, actor) VALUES ('2026-10-27T11:30:00Z', 'smashcraft#62', 'note', 'w62')", []).unwrap();
    let (code, out, _) = ledger.run(&["claim", "smashcraft#61", "--by", "sc-61-w"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("sc-61-w owns smashcraft#61, taken over from w61: stale, no owner event since 2026-10-27T10:40:00Z"), "{out}");
    let (code, _, err) = ledger.run(&["claim", "smashcraft#62", "--by", "sc-62-w"]);
    assert_eq!((code, err.as_str()), (1, "NorthThreads: smashcraft#62 is already owned by w62\n"));
}

#[test]
fn list_all_shows_the_void_claims_an_exited_owner_left_and_the_plain_list_still_omits_them() {
    let mut ledger = ledger(&[(796, Issue::Open { opened: at("2026-10-01T00:00:00Z") })]);
    ledger.alive = Some(BTreeSet::new());
    ledger.claimed("smashcraft#796", "sc-796-d0921", "2026-10-30T09:21:46Z", None);
    assert!(ledger.listed().is_empty());
    assert!(ledger.dispatch_holds().is_empty());
    let (code, listing, _) = ledger.run(&["list", "--json", "--all"]);
    let rows: Value = serde_json::from_str(&listing).unwrap();
    assert_eq!((code, rows[0]["item"].as_str(), rows[0]["owner"].as_str(), rows[0]["standing"].as_str()), (0, Some("smashcraft#796"), Some("sc-796-d0921"), Some("void")));
    assert_eq!(ledger.run(&["list", "--all"]).0, 1);
}
