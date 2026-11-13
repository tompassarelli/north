use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::threads::claims::issues::{self, Issues};
use crate::threads::claims::verbs::{self, Context};
use crate::threads::claims::Issue;
use crate::foundation::clock::Timestamp;
use super::survey::claim_rows;

const SCHEMA: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL, actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE TABLE claims(item TEXT PRIMARY KEY, owner TEXT NOT NULL, since TEXT NOT NULL, eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)) STRICT;
CREATE TABLE items(ns TEXT NOT NULL, number INTEGER NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'open', created TEXT NOT NULL, closed TEXT, due TEXT, PRIMARY KEY(ns, number)) STRICT;";
const NOW: &str = "2026-10-27T12:00:00Z";

struct Seen(BTreeMap<(String, u64), Issue>, BTreeMap<(String, u64), String>);

impl Issues for Seen {
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue> {
        items.iter().filter_map(|item| Some((item.clone(), *self.0.get(item)?))).collect()
    }

    fn with_body(&self, repo: &str, number: u64) -> (Issue, Option<String>) {
        let item = (repo.to_owned(), number);
        (self.0.get(&item).copied().unwrap_or(Issue::Unread), self.1.get(&item).cloned())
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
    let directory = tempfile::tempdir_in("/dev/shm").or_else(|_| tempfile::tempdir()).unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA).unwrap();
    let issues = Seen(issues.iter().map(|(number, issue)| (("makgora".to_owned(), *number), *issue)).collect(), BTreeMap::new());
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
        let context = Context { database: &self.path, actor: "tester".into(), no_native: self.no_native, issues: &self.issues, alive: self.alive.clone(), now: at(NOW), replica: false };
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

    fn owner(&self, item: &str) -> String {
        self.db().query_row("SELECT owner FROM claims WHERE item = ?1", [item], |row| row.get(0)).unwrap()
    }
}

#[test]
fn a_claim_made_before_its_issue_opened_is_ignored_by_claim_list_and_dispatch() {
    let ledger = ledger(&[(523, Issue::Open { opened: at("2026-10-26T08:00:00Z") })]);
    ledger.claimed("makgora#523", "art-a8f12c1a", "2026-10-12T09:00:00Z", Some(60));
    assert!(ledger.listed().is_empty());
    assert!(ledger.dispatch_holds().is_empty());
    let (code, out, err) = ledger.run(&["claim", "makgora#523", "--by", "sc-523-w", "--eta", "45"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "sc-523-w owns makgora#523; the claim by art-a8f12c1a was void because it was made before its issue was opened\n");
    assert_eq!(ledger.owner("makgora#523"), "sc-523-w");
    assert_eq!(ledger.dispatch_holds(), ["makgora#523"]);
}

#[test]
fn github_answers_a_missing_issue_beside_the_found_ones_and_an_unread_reply_voids_nothing() {
    let items: BTreeSet<(String, u64)> = [("makgora".to_owned(), 453), ("makgora".to_owned(), 4824), ("wisp".to_owned(), 7)].into();
    assert_eq!(
        issues::query(&items, issues::STANDING),
        "query { r0: repository(owner: \"tompassarelli\", name: \"makgora\") { i453: issue(number: 453) { state createdAt } i4824: issue(number: 4824) { state createdAt } } r1: repository(owner: \"tompassarelli\", name: \"wisp\") { i7: issue(number: 7) { state createdAt } } }"
    );
    let reply = serde_json::json!({"data": {"r0": {"i453": {"state": "CLOSED", "createdAt": "2026-10-26T08:09:49Z"}, "i4824": null}, "r1": {"i7": {"state": "OPEN", "createdAt": "2026-10-01T00:00:00Z"}}}});
    let found = issues::answers(&items, &reply);
    assert_eq!(found.values().copied().collect::<Vec<_>>(), [Issue::Closed, Issue::Missing, Issue::Open { opened: at("2026-10-01T00:00:00Z") }]);
    assert!(issues::answers(&items, &Value::Null).values().all(|issue| *issue == Issue::Unread));
}

/// On 2026-11-07 the proxy split north#782 into north#783 and north#784 to staff three workers on it, and n-loss-lead launched without --task to get past "already owned".
#[test]
fn three_workers_claim_three_boxes_of_one_issue_while_a_second_claim_on_one_box_or_on_the_whole_issue_is_refused() {
    let ledger = ledger(&[(782, Issue::Open { opened: at("2026-10-26T08:00:00Z") })]);
    let boxes = ["- [ ] the loss is classified", "- [ ] the run row names its box", "- [ ] the staffing replays"];
    for (worker, text) in ["n-loss", "n-run-row", "n-replay"].into_iter().zip(boxes) {
        let (code, out, err) = ledger.run(&["claim", "makgora#782", "--box", text, "--by", worker]);
        assert_eq!((code, err.as_str()), (0, ""), "{worker}");
        assert!(out.starts_with(&format!("{worker} owns makgora#782 box ")), "{out}");
    }
    let (code, _, err) = ledger.run(&["claim", "makgora#782", "--box", "The loss is classified", "--by", "n-late"]);
    assert_eq!(code, 1);
    assert_eq!(err, "NorthThreads: makgora#782 box the-loss-is-classified is already owned by n-loss\n");
    let (code, _, err) = ledger.run(&["claim", "makgora#782", "--by", "n-late"]);
    assert_eq!((code, err.as_str()), (1, "NorthThreads: makgora#782 is already owned by n-loss\n"), "the whole issue is refused beside live box claims");
    let (code, out, _) = ledger.run(&["claim", "makgora#782", "--by", "n-run-row"]);
    assert_eq!((code, out.as_str()), (0, "n-run-row owns makgora#782 box the-run-row-names-its-box\n"), "a worker holding a box that claims the issue renews its box");
    assert!(ledger.dispatch_holds().is_empty(), "box claims hold boxes, not the issue");
    let (_, listing, _) = ledger.run(&["list", "--json"]);
    let boxed = super::survey::claim_box_rows(&serde_json::from_str::<Value>(&listing).unwrap(), "nexus", &Default::default());
    assert_eq!(boxed.iter().map(|(_, slug, _)| slug.as_str()).collect::<Vec<_>>(), ["the-loss-is-classified", "the-run-row-names-its-box", "the-staffing-replays"]);
    ledger.db().execute("DELETE FROM claims", []).unwrap();
    assert_eq!(ledger.run(&["claim", "makgora#782", "--by", "n-late"]).0, 0, "the whole issue is claimable once every box claim is gone");
}

/// On 2026-11-11 gex45-n1109-mirror launched with `--task north#1109` and a box of north#872, so its claim named no box of north#1109 and dispatch held all thirteen of its boxes; north#1020 was held the same way at 22:20 UTC on 2026-11-10.
#[test]
fn a_claim_on_a_box_its_issue_lacks_is_refused_and_one_on_its_own_box_holds_only_that_box() {
    let mut ledger = ledger(&[(1109, Issue::Open { opened: at("2026-10-26T08:00:00Z") })]);
    let body = "- [ ] north-fleet: no test in crates/north-fleet waits on the clock\n- [x] north-base: no test in crates/north-base waits on the clock\n- [ ] north-ops: no test in crates/north-ops waits on the clock";
    ledger.issues.1.insert(("makgora".to_owned(), 1109), body.to_owned());
    let (code, _, err) = ledger.run(&["claim", "makgora#1109", "--box", "Dispatch on nexus sees every issue filed since its token broke", "--by", "n1109-mirror"]);
    assert_eq!(code, 1);
    assert!(err.contains("names no box of makgora#1109") && err.contains("north-fleet: no test in crates/north-fleet") && err.contains("north-base: no test in crates/north-base"), "the refusal names the issue's box texts: {err}");
    assert!(ledger.listed().is_empty(), "a refused claim holds nothing");
    for (worker, text) in [("n-fleet", "north-fleet: no test"), ("n-base", "north-base: no test in crates/north-base waits on the clock")] {
        let (code, _, err) = ledger.run(&["claim", "makgora#1109", "--box", text, "--by", worker]);
        assert_eq!((code, err.as_str()), (0, ""), "{worker}: a box's first words and a ticked box are boxes of the issue");
    }
    assert!(ledger.dispatch_holds().is_empty(), "box claims hold boxes, not the issue");
    ledger.issues.1.clear();
    assert_eq!(ledger.run(&["claim", "makgora#1109", "--box", "a box GitHub could not show", "--by", "n-unread"]).0, 0, "an unread body refuses nothing");
}
