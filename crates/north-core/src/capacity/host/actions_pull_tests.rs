use rusqlite::Connection;

use super::*;
use crate::capacity::host::database::{SCHEMA, add_run_columns};

fn fixture_database() -> Connection {
    let db = Connection::open_in_memory().expect("open");
    db.execute_batch(SCHEMA).expect("schema");
    add_run_columns(&db).expect("run columns");
    db
}

const JOBS: &str = concat!(
    r#"{"id":11,"name":"sheriff","workflow_name":"CI","labels":["ubuntu-latest"],"created_at":"2026-11-03T23:58:55Z","started_at":"2026-11-03T23:58:58Z","completed_at":"2026-11-03T23:59:08Z","conclusion":"success"}"#,
    "\n",
    r#"{"id":12,"name":"balance","workflow_name":"CI","labels":["self-hosted","Farm-Big"],"created_at":"2026-11-03T23:58:55Z","started_at":"2026-11-03T23:59:55Z","completed_at":"2026-11-04T00:09:55Z","conclusion":"failure"}"#,
    "\n",
    r#"{"id":13,"name":"skipped","workflow_name":"CI","labels":["ubuntu-latest"],"created_at":"2026-11-03T23:58:55Z","started_at":null,"completed_at":null,"conclusion":"skipped"}"#,
    "\n",
);

fn now() -> Timestamp {
    Timestamp::parse_rfc3339("2026-11-04T01:00:00Z").expect("time")
}

#[test]
fn a_run_records_each_started_job_with_its_runner_label_queue_and_run_time_and_a_second_pull_adds_none() {
    let mut db = fixture_database();
    let asked = std::cell::RefCell::new(Vec::new());
    let gh = |endpoint: &str, _filter: &str, _paginate: bool| -> Result<String, String> {
        asked.borrow_mut().push(endpoint.to_owned());
        Ok(if endpoint.contains("/jobs") { JOBS.to_owned() } else { "900\n".to_owned() })
    };
    assert_eq!(pull_repo(&mut db, "north", now(), &gh), Ok(2));
    let rows: Vec<(String, f64, f64, String, i64)> = db
        .prepare("SELECT host, queuedSeconds, wallSeconds, shape, exitCode FROM capacity_runs ORDER BY source_id")
        .expect("query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .expect("rows")
        .flatten()
        .collect();
    assert_eq!(
        rows,
        vec![("actions:ubuntu-latest".to_owned(), 3.0, 10.0, "success".to_owned(), 0), ("actions:farm-big".to_owned(), 60.0, 600.0, "failure".to_owned(), 1)]
    );
    let repo: String = db.query_row("SELECT repo FROM capacity_runs LIMIT 1", [], |row| row.get(0)).expect("repo");
    assert_eq!(repo, "north");
    let calls = asked.borrow().len();
    assert_eq!(pull_repo(&mut db, "north", now(), &gh), Ok(0));
    assert_eq!(asked.borrow().len(), calls + 1, "the known run's jobs are not fetched again");
}

#[test]
fn the_first_pull_looks_back_a_week_and_later_ones_start_a_day_before_the_last_job() {
    let mut db = fixture_database();
    let asked = std::cell::RefCell::new(Vec::new());
    let gh = |endpoint: &str, _filter: &str, _paginate: bool| -> Result<String, String> {
        asked.borrow_mut().push(endpoint.to_owned());
        Ok(if endpoint.contains("/jobs") { JOBS.to_owned() } else { "900\n".to_owned() })
    };
    pull_repo(&mut db, "wisp", now(), &gh).expect("first");
    pull_repo(&mut db, "wisp", now(), &gh).expect("second");
    let listings: Vec<String> = asked.borrow().iter().filter(|endpoint| !endpoint.contains("/jobs")).cloned().collect();
    assert!(listings[0].contains("created=%3E%3D2026-10-28"), "{}", listings[0]);
    assert!(listings[1].contains("created=%3E%3D2026-11-03"), "{}", listings[1]);
}

#[test]
fn the_median_queue_prints_per_runner_label() {
    let db = fixture_database();
    for (index, queue) in [4.0, 10.0, 100.0].into_iter().enumerate() {
        db.execute(
            "INSERT INTO capacity_runs (source_id, at, family, class, signature_exact, host, queuedSeconds) VALUES (?, '2026-11-04T00:00:00Z', 'actions', 'ci', 0, 'actions:farm-big', ?)",
            rusqlite::params![format!("q{index}"), queue],
        )
        .expect("row");
    }
    db.execute("INSERT INTO capacity_runs (source_id, at, family, class, signature_exact, host, queuedSeconds) VALUES ('h', '2026-11-04T00:00:00Z', 'actions', 'ci', 0, 'actions:ubuntu-latest', 2)", []).expect("row");
    db.execute("INSERT INTO capacity_runs (source_id, at, family, class, signature_exact, host, queuedSeconds) VALUES ('g', '2026-11-04T00:00:00Z', 'cargo', 'heavy', 1, 'gex45', 999)", []).expect("row");
    assert_eq!(queue_p50_by_label(&db).expect("p50"), vec![("actions:farm-big".to_owned(), 3, 10.0), ("actions:ubuntu-latest".to_owned(), 1, 2.0)]);
}

#[test]
fn a_failing_gh_call_adds_nothing_and_says_why() {
    let mut db = fixture_database();
    let gh = |_: &str, _: &str, _: bool| -> Result<String, String> { Err("gh api: HTTP 403".to_owned()) };
    assert_eq!(pull_repo(&mut db, "north", now(), &gh), Err("gh api: HTTP 403".to_owned()));
}

#[test]
fn a_red_main_prints_an_incident_line_naming_the_workflow_and_a_green_or_cancelled_one_prints_none() {
    let red = serde_json::json!({"name": "Land", "conclusion": "failure", "updated_at": "2026-11-04T00:10:00Z", "html_url": "https://github.com/tompassarelli/north/actions/runs/9"});
    assert_eq!(
        main_incident("north", &red),
        Some("incident: north main is red: Land concluded failure at 2026-11-04T00:10:00Z https://github.com/tompassarelli/north/actions/runs/9".to_owned())
    );
    for conclusion in ["success", "cancelled", "skipped"] {
        assert_eq!(main_incident("north", &serde_json::json!({"name": "Land", "conclusion": conclusion})), None);
    }
}

#[test]
fn the_latest_main_run_is_read_unpaginated_from_push_events_only() {
    let asked = std::cell::RefCell::new(Vec::new());
    let gh = |endpoint: &str, _filter: &str, paginate: bool| -> Result<String, String> {
        asked.borrow_mut().push((endpoint.to_owned(), paginate));
        Ok(r#"{"name":"Land","conclusion":"failure"}"#.to_owned())
    };
    let run = latest_main_run("wisp", &gh).expect("run").expect("a run");
    assert_eq!(run["conclusion"], "failure");
    let asked = asked.borrow();
    assert!(asked[0].0.contains("repos/tompassarelli/wisp/actions/runs?branch=main&event=push&status=completed&per_page=1"));
    assert!(!asked[0].1);
}
