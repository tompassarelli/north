use std::sync::Mutex;

use rusqlite::Connection;

use super::super::decide::Decision;
use super::super::decide::world::{
    EstimateSource, Expected, Label, ModelFamily, Name, Owner, Session, Standing, Unix, Usd, Workstream,
};
use super::{Launch, Registry, Shell, brief, dry_run};
use crate::foundation::clock::Timestamp;

const CREATE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";
const HEADERS: &str =
    "Messages response anthropic-ratelimit-unified headers on the account's setup token";
const BRIEF: &str = "Item: north#259 (box: Usage box 5)\nTier: sonnet-high\nETA: 30 min\nBudget: E p50 $3.37, p90 $6.74 (p50 and p90 of n=82 closed boxes); `north fleet spend --self` prints your API-equivalent spend so far; at $6.74 API-equivalent spend, commit and land\n";

struct Fixture {
    directory: tempfile::TempDir,
    db: Connection,
    registry: Registered,
}

#[derive(Default)]
struct Registered(Mutex<Vec<Session>>);

impl Registry for Registered {
    fn sessions(&self) -> Result<Vec<Session>, String> {
        Ok(self.0.lock().unwrap().clone())
    }
}

impl Fixture {
    fn new() -> Self {
        let db = Connection::open_in_memory().unwrap();
        db.execute(CREATE, []).unwrap();
        for (account, five_hour, week) in [("a", 10.0, 20.0), ("b", 40.0, 50.0)] {
            for (name, used, minutes, resets) in [
                ("session", five_hour, 300, "2026-11-05T16:00:00Z"),
                ("weekly_all", week, 10080, "2026-11-10T00:00:00Z"),
            ] {
                db.execute(
                    "INSERT INTO usage VALUES ('2026-11-05T11:59:00Z', 'claude', ?1, 'window', ?2, ?3, ?4, ?5, NULL, ?6)",
                    rusqlite::params![account, name, used, minutes, resets, HEADERS],
                )
                .unwrap();
            }
        }
        Self {
            directory: tempfile::tempdir().unwrap(),
            db,
            registry: Registered::default(),
        }
    }

    fn shell(&self) -> Shell<'_> {
        Shell {
            db: &self.db,
            policy: std::path::Path::new("/nonexistent/orchestration.toml"),
            host: "laptop",
            registry: &self.registry,
            directory: self.directory.path().join("usage"),
        }
    }
}

fn now() -> Unix {
    Unix(
        Timestamp::parse_rfc3339("2026-11-05T12:00:00Z")
            .unwrap()
            .unix_seconds(),
    )
}

fn session(name: &str, account: &Label) -> Session {
    Session {
        name: Name(name.to_owned()),
        owner: Owner::Fleet,
        account: account.clone(),
        family: ModelFamily::Sonnet,
        workstream: Some(Workstream("north#259".to_owned())),
        box_id: None,
        standing: Standing::Worker,
        spent: None,
        active_minutes: 0.0,
        expected: Expected {
            p50: Usd(3.37),
            p90: Usd(6.74),
            minutes: 30.0,
            source: EstimateSource::None,
        },
        parked_since: None,
    }
}

#[test]
fn the_second_decision_lists_the_first_in_flight_under_the_lock() {
    let fixture = Fixture::new();
    let shell = fixture.shell();
    let request = brief::request(BRIEF, "laptop");
    let record = |name: &'static str| {
        let registry = &fixture.registry;
        move |account: &Label| -> Result<(), String> {
            registry.0.lock().unwrap().push(session(name, account));
            Ok(())
        }
    };

    let first = shell.decide(now(), &request, &record("first")).unwrap();
    let Decision::Place { account, .. } = &first.decision else {
        panic!("first launch refused: {}", first.line)
    };
    assert!(first.in_flight.is_empty());

    let second = shell.decide(now(), &request, &record("second")).unwrap();
    assert!(
        matches!(second.decision, Decision::Place { .. }),
        "{}",
        second.line
    );
    assert_eq!(second.in_flight, ["first"]);
    assert_eq!(fixture.registry.0.lock().unwrap()[0].account, *account);

    let logged =
        std::fs::read_to_string(fixture.directory.path().join("usage/decisions.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = logged
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["in_flight"], serde_json::json!(["first"]));
    assert_eq!(lines[1]["decision"], second.line);
}

#[test]
fn a_dry_run_records_nothing_and_prints_the_explain_line_and_the_launches_in_flight() {
    let fixture = Fixture::new();
    let shell = fixture.shell();
    fixture
        .registry
        .0
        .lock()
        .unwrap()
        .push(session("running", &Label("a".to_owned())));

    let text = dry_run(&shell, BRIEF).unwrap();
    let mut lines = text.lines();
    assert!(lines.next().unwrap().starts_with("place "), "{text}");
    assert_eq!(lines.next(), Some("in flight: running"));
    assert!(
        !fixture
            .directory
            .path()
            .join("usage/decisions.jsonl")
            .exists()
    );
}

#[test]
fn agent_tool_launches_are_in_flight_until_their_record_is_removed() {
    let fixture = Fixture::new();
    let shell = fixture.shell();
    let log = shell.launch_log();
    std::fs::create_dir_all(fixture.directory.path().join("usage")).unwrap();
    log.add(&Launch {
        tool_use_id: "toolu_1".into(),
        name: "agent-1".into(),
        account: "a".into(),
        family: "sonnet".into(),
        workstream: None,
        p50: 1.0,
        p90: 2.0,
        minutes: 10.0,
    })
    .unwrap();

    let during = shell.world(now()).unwrap();
    assert_eq!(during.sessions.len(), 1);
    assert_eq!(
        during.sessions[0].owner,
        Owner::AgentTool {
            tool_use_id: "toolu_1".into()
        }
    );

    log.remove("toolu_1").unwrap();
    assert!(shell.world(now()).unwrap().sessions.is_empty());
}

#[test]
fn a_brief_states_the_family_the_workstream_and_the_expected_spend() {
    let request = brief::request(BRIEF, "laptop");
    assert_eq!(request.family, ModelFamily::Sonnet);
    assert_eq!(request.workstream, Some(Workstream("north#259".to_owned())));
    assert_eq!(
        (
            request.expected.p50,
            request.expected.p90,
            request.expected.minutes
        ),
        (Usd(3.37), Usd(6.74), 30.0)
    );
}

#[test]
fn a_limit_row_in_the_usage_table_is_a_limit_event_in_the_world() {
    let fixture = Fixture::new();
    fixture
        .db
        .execute(
            "INSERT INTO usage VALUES ('2026-11-05T11:58:00Z', 'claude', 'a', 'limit', 'weekly_all', 100.0, NULL, '2026-11-10T00:00:00Z', NULL, 'claude stream rate_limit_event; rejected')",
            [],
        )
        .unwrap();
    let parts = super::assemble::parts(&fixture.db, std::path::Path::new("/nonexistent/orchestration.toml"), "laptop", now(), Vec::new()).unwrap();
    let at = |text: &str| Unix(Timestamp::parse_rfc3339(text).unwrap().unix_seconds());
    assert_eq!(
        parts.limits,
        [super::super::decide::world::LimitEvent {
            account: Label("a".to_owned()),
            window: super::super::decide::world::Window::Week,
            resets_at: Some(at("2026-11-10T00:00:00Z")),
            at: at("2026-11-05T11:58:00Z"),
        }]
    );
}
