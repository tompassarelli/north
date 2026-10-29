use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::Connection;
use serde_json::json;

use super::branches::names_issue;
use super::{comment_block, run};
use crate::foundation::clock::Timestamp;
use crate::threads::fake::Recorded;
use crate::threads::verb::Context;

const SCHEMA: &str = "CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT NOT NULL, item TEXT, kind TEXT NOT NULL, actor TEXT NOT NULL DEFAULT '', tier TEXT NOT NULL DEFAULT '', data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT;
CREATE TABLE claims(item TEXT PRIMARY KEY CHECK(item GLOB '*#[0-9]*'), owner TEXT NOT NULL CHECK(length(owner) > 0), since TEXT NOT NULL, eta_min INTEGER CHECK(eta_min IS NULL OR eta_min > 0)) STRICT;
CREATE TABLE blocks(item TEXT PRIMARY KEY, reason TEXT NOT NULL, since TEXT NOT NULL, by TEXT NOT NULL) STRICT;
CREATE TABLE items(ns TEXT NOT NULL, number INTEGER NOT NULL CHECK(number > 0), title TEXT NOT NULL CHECK(length(title) > 0), body TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'open' CHECK(state IN ('open','closed')), created TEXT NOT NULL, closed TEXT, due TEXT, PRIMARY KEY(ns, number)) STRICT;";
const NOW: &str = "2026-10-28T09:00:00Z";

static COMPARED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"b(\d+): compare\(headRef: "([^"]+)"\)"#).unwrap());

fn issue(number: u64, priority: Option<&str>, body: &str, comments: serde_json::Value) -> serde_json::Value {
    let labels: Vec<_> = priority.map(|priority| json!({"name": format!("priority:{priority}")})).into_iter().collect();
    json!({"number": number, "title": format!("T{number}"), "labels": labels, "body": body, "comments": comments})
}

fn branch(name: &str, committed: &str, status: serde_json::Value) -> serde_json::Value {
    json!({"name": name, "target": {"committedDate": committed, "status": {"context": status}}})
}

/// smashcraft's open issues, branches and lanes, wisp's one issue; a deleted branch refuses the
/// first compare.
fn github(joined: &str) -> Option<Result<String, String>> {
    let reply = if joined.contains("issue list -R tompassarelli/smashcraft") {
        json!([
            issue(15, Some("next"), "- [ ] a", json!([])),
            issue(16, None, "- [ ] a", json!([])),
            issue(17, Some("now"), "- [x] done", json!([])),
            issue(18, Some("later"), "- [ ] a", json!([])),
            issue(10, Some("now"), "- [ ] a\n- [ ] b", json!([])),
            issue(11, Some("now"), "- [ ] a", json!([])),
            issue(12, Some("now"), "- [ ] a", json!([])),
            issue(13, Some("now"), "- [ ] a", json!([{"body": "Blocked: waits on Tom\nuntil Friday", "createdAt": NOW}])),
            issue(14, Some("now"), "* [ ] a", json!([])),
        ])
    } else if joined.contains("issue list -R tompassarelli/wisp") {
        json!([issue(20, Some("now"), "- [ ] a", json!([]))])
    } else if joined.contains("refPrefix") && joined.ends_with("name=smashcraft") {
        json!({"data": {"repository": {"refs": {"nodes": [
            branch("main", NOW, json!(null)),
            branch("claude/fix-12", NOW, json!(null)),
            branch("claude/other", NOW, json!(null)),
            branch("claude/gone", NOW, json!(null)),
            branch("claude/fix-14", NOW, json!({"state": "FAILURE", "description": "tests failed", "createdAt": "2026-10-28T08:30:00Z"})),
            branch("claude/fix-15", "2026-10-20T00:00:00Z", json!(null)),
            branch("smashcraft-land-16", NOW, json!({"state": "FAILURE", "description": null, "createdAt": null})),
        ]}}}})
    } else if joined.contains("refPrefix") {
        json!({"data": {"repository": {"refs": {"nodes": []}}}})
    } else if joined.contains("compare(headRef") {
        if joined.contains("claude/gone") {
            return Some(Err("gh api graphql: GraphQL: Could not resolve head ref 'claude/gone' (repository.defaultBranchRef.b2)".into()));
        }
        let mut base = serde_json::Map::new();
        for found in COMPARED.captures_iter(joined) {
            let message = match &found[2] {
                "claude/other" => "Tidy\n\nUnlike #14, this leaves the ledger alone.",
                "smashcraft-land-16" => "Port the thing\n\nItem: smashcraft#16",
                _ => "Fix it",
            };
            base.insert(format!("b{}", &found[1]), json!({"commits": {"nodes": [{"message": message}]}}));
        }
        json!({"data": {"repository": {"defaultBranchRef": base}}})
    } else {
        return None;
    };
    Some(Ok(reply.to_string()))
}

struct Ledger {
    _directory: tempfile::TempDir,
    path: PathBuf,
    outside: Recorded,
}

fn ledger() -> Ledger {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    Connection::open(&path).unwrap().execute_batch(SCHEMA).unwrap();
    Ledger { _directory: directory, path, outside: Recorded { respond: Some(github), alive: vec!["alice", "bob", "dave"], ..Recorded::default() } }
}

impl Ledger {
    fn exec(&self, sql: &str) {
        Connection::open(&self.path).unwrap().execute_batch(sql).unwrap();
    }

    fn sql(&self, query: &str) -> String {
        Connection::open(&self.path).unwrap().query_row(query, [], |row| row.get(0)).unwrap()
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        let context = Context { database: &self.path, actor: "tester".into(), no_native: None, outside: &self.outside, now: Timestamp::parse(NOW).unwrap(), replica: false };
        let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&argv, &context, &mut &b""[..], &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }
}

#[test]
fn unowned_ranks_priority_now_first_and_names_each_owner_branch_or_block() {
    let rig = ledger();
    rig.exec("INSERT INTO claims VALUES ('smashcraft#11', 'alice', '2026-10-28T08:00:00Z', 30), ('smashcraft#16', 'dave', '2026-10-28T08:50:00Z', NULL);
        INSERT INTO blocks VALUES ('wisp#20', 'waits on smashcraft#13', '2026-10-28T08:00:00Z', 'lead')");
    let (code, out, err) = rig.run(&["unowned", "--by", "lead"]);
    assert_eq!((code, err.as_str()), (0, "NorthThreads: skipped deleted smashcraft branches claude/gone\n"));
    assert_eq!(
        out,
        "smashcraft#10\tpriority:now\tUNOWNED\t2\tT10\n\
         smashcraft#11\tpriority:now\talice\t1\tT11\n\
         smashcraft#12\tpriority:now\tbranch:claude/fix-12\t1\tT12\n\
         smashcraft#13\tpriority:now\tblocked:waits on Tom\t1\tT13\n\
         smashcraft#14\tpriority:now\tUNOWNED\t1\tT14\tlane failed: tests failed (claude/fix-14)\n\
         wisp#20\tpriority:now\tblocked:waits on smashcraft#13\t1\tT20\n\
         smashcraft#16\t-\tdave\t1\tT16\n\
         smashcraft#15\tpriority:next\tUNOWNED\t1\tT15\n\
         smashcraft#18\tpriority:later\tUNOWNED\t1\tT18\n\
         priority:now unowned=2 blocked=2 owned=2\n"
    );
    let calls: Vec<String> = rig.outside.calls.borrow().iter().map(|call| call.join(" ")).collect();
    assert_eq!(calls[0], "issue list -R tompassarelli/smashcraft --state open --limit 500 --json number,title,labels,body,comments");
    assert!(calls[1].starts_with("api graphql -f query=query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { refs(refPrefix: \"refs/heads/\", first: 100)"));
    assert!(calls[3].contains("{ defaultBranchRef { b0: compare(headRef: \"claude/fix-12\") { commits(first: 100) { nodes { message } } } b1: compare(headRef: \"claude/other\")"), "{}", calls[3]);
    assert_eq!(calls.len(), 6);
}

#[test]
fn a_lane_that_failed_autoland_releases_the_claim_made_before_it_and_is_named_on_its_unowned_issue() {
    let rig = ledger();
    rig.exec("INSERT INTO claims VALUES ('smashcraft#14', 'bob', '2026-10-28T08:00:00Z', NULL), ('smashcraft#16', 'dave', '2026-10-28T08:50:00Z', NULL)");
    let out = rig.run(&["unowned"]).1;
    assert!(out.contains("smashcraft#14\tpriority:now\tUNOWNED\t1\tT14\tlane failed: tests failed (claude/fix-14)\n"), "{out}");
    assert!(out.contains("smashcraft#16\t-\tdave\t1\tT16\n"), "{out}");
    assert_eq!(rig.sql("SELECT group_concat(item || ' ' || owner) FROM claims"), "smashcraft#16 dave");
    assert_eq!(
        rig.sql("SELECT group_concat(item || ' ' || kind || ' ' || actor || ' ' || data, char(10)) FROM events"),
        "smashcraft#14 release threads {\"owner\": \"bob\", \"lane_failed\": \"claude/fix-14\", \"reason\": \"tests failed\", \"failed_at\": \"2026-10-28T08:30:00Z\"}"
    );
    rig.exec("DELETE FROM claims");
    assert!(rig.run(&["unowned"]).1.contains("smashcraft#16\t-\tUNOWNED\t1\tT16\tlane failed: no reason given (smashcraft-land-16)\n"));
}

#[test]
fn a_branch_or_commit_names_an_issue_by_its_name_headline_or_reference_line_but_not_a_passing_mention() {
    let lines = |text: &str| vec![text.to_owned()];
    assert!(names_issue("claude/fix-12", &[], "smashcraft", 12));
    assert!(names_issue("land-12-abc", &[], "smashcraft", 12) && names_issue("claude/12", &[], "smashcraft", 12));
    assert!(!names_issue("claude/fix-123", &[], "smashcraft", 12) && !names_issue("claude/v12x", &[], "smashcraft", 12));
    assert!(names_issue("claude/x", &lines("Fix the ledger (#12)"), "smashcraft", 12));
    assert!(names_issue("claude/x", &lines("Tidy\n\n  Closes smashcraft#12"), "smashcraft", 12));
    assert!(names_issue("claude/x", &lines("Tidy\r\nPart of #12"), "smashcraft", 12));
    assert!(!names_issue("claude/x", &lines("Tidy\n\nUnlike #12, this stays."), "smashcraft", 12));
    assert!(!names_issue("claude/x", &lines("See wisp#12 and #120 and a-#12 and x/#12"), "smashcraft", 12));
}

#[test]
fn a_comment_block_is_the_newest_blocked_comment_of_the_week_and_an_older_comment_ends_the_search() {
    let now = Timestamp::parse(NOW).unwrap();
    let comment = |body: &str, at: &str| json!({"body": body, "createdAt": at});
    let recent = [comment("Blocked: old reason", "2026-10-27T00:00:00Z"), comment("  Blocked:   waits on Tom \nmore", "2026-10-28T08:00:00Z")];
    assert_eq!(comment_block(&recent, now).as_deref(), Some("waits on Tom "));
    assert_eq!(comment_block(&[comment("Blocked:", "2026-10-28T08:00:00Z")], now).as_deref(), Some("comment"));
    assert_eq!(comment_block(&[comment("fine now", "2026-10-28T08:00:00Z"), comment("Blocked: x", "2026-10-27T00:00:00Z")], now).as_deref(), Some("x"));
    assert_eq!(comment_block(&[comment("Blocked: stale", "2026-10-20T00:00:00Z")], now), None);
    assert_eq!(comment_block(&[comment("noted", "2026-10-19T00:00:00Z"), comment("Blocked: x", "2026-10-20T00:00:00Z")], now), None);
}
