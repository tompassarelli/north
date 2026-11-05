use std::collections::BTreeSet;

use rusqlite::{Connection, params};

use super::predictions::{Brier, Source, record, score_with};
use crate::foundation::clock::Timestamp;
use crate::threads::runs::brief::Fields;

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn ledger(directory: &std::path::Path) -> (std::path::PathBuf, Connection) {
    let database = directory.join("threads.db");
    let db = Connection::open(&database).unwrap();
    db.execute_batch(include_str!("../runs/fixtures/ledger.sql")).unwrap();
    db.execute_batch("CREATE TABLE calibration(at TEXT NOT NULL, kind TEXT NOT NULL, cell TEXT NOT NULL, n INTEGER NOT NULL, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT NOT NULL) STRICT; CREATE TABLE tier_choice(cell TEXT PRIMARY KEY, tier TEXT NOT NULL, since TEXT NOT NULL) STRICT;").unwrap();
    (database, db)
}

fn recommend(database: &std::path::Path, configuration: &std::path::Path, argv: &[&str], facets: &str) -> String {
    let outside = crate::threads::fake::Recorded::default();
    let context = crate::threads::verb::Context { database, actor: "tester".into(), no_native: None, outside: &outside, now: at("2026-10-10T00:00:00Z"), replica: false };
    let argv: Vec<String> = argv.iter().map(|word| (*word).to_owned()).collect();
    let args = crate::threads::verb::Args::parse(&argv, &["--since", "--launched"], &[]).ok().unwrap();
    match super::verbs::recommend_with(&args, &context, &Fields::read(facets), configuration) {
        Ok(text) => text,
        Err(_) => panic!("recommend refused"),
    }
}

fn predictions(db: &Connection) -> Vec<(String, String, String, f64, String, String)> {
    let mut statement = db.prepare("SELECT item, tier, category, p, at, source FROM tier_predictions ORDER BY item, tier").unwrap();
    statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn recommend_lists_no_codex_tier_and_stores_the_shrunk_success_of_each_listed_tier_for_an_items_first_run() {
    let directory = tempfile::tempdir().unwrap();
    let (database, db) = ledger(directory.path());
    for (index, (tier, outcome)) in [("claude:haiku", "done"), ("claude:haiku", "done"), ("claude:haiku", "done"), ("claude:haiku", "done"), ("claude:haiku", "not done")].into_iter().chain([("codex:luna-low", "done"); 5]).enumerate() {
        db.execute(
            "INSERT INTO runs(agent, tier, category, ended, minutes, outcome, role) VALUES (?1, ?2, 'mechanical', ?3, 10, ?4, 'worker')",
            params![format!("a{index}"), tier, format!("2026-10-0{}T00:00:00Z", index % 9 + 1), outcome],
        )
        .unwrap();
    }
    db.execute("INSERT INTO tier_choice(cell, tier, since) VALUES ('Category: mechanical', 'codex:luna-low', '2026-10-01T00:00:00Z')", []).unwrap();
    let configuration = directory.path().join("orchestration.toml");
    std::fs::write(&configuration, "[tiers.\"claude:haiku\"]\nstart = 0\n[tiers.\"codex:luna-low\"]\nstart = 0\n").unwrap();

    let text = recommend(&database, &configuration, &[], "Item: north#900\nCategory: mechanical");
    assert!(!text.contains("codex:"), "{text}");
    assert!(text.starts_with("tier claude:haiku  cell [Category: mechanical]  n 5\n"), "{text}");
    assert!(text.contains("  claude:haiku       80% of 5  shrunk 80%"), "{text}");
    let shrunk = (4.0 + 15.0 * 0.8) / 20.0;
    assert_eq!(predictions(&db), [("north#900".into(), "claude:haiku".into(), "mechanical".into(), shrunk, "2026-10-10T00:00:00Z".into(), "recommend".into())]);

    recommend(&database, &configuration, &["--launched", "claude:sonnet-high"], "Item: north#901\nCategory: mechanical");
    recommend(&database, &configuration, &[], "Item: north#902\nFollows: worker-north-902\nCategory: mechanical");
    recommend(&database, &configuration, &[], "Category: mechanical");
    let stored = predictions(&db);
    assert_eq!(stored.len(), 2, "a retry and a brief with no Item store nothing: {stored:?}");
    assert_eq!(stored[1], ("north#901".into(), "claude:sonnet-high".into(), "mechanical".into(), 0.8, "2026-10-10T00:00:00Z".into(), "dispatch".into()), "an unmeasured launched tier predicts the category's pooled rate");
}

#[test]
fn the_score_takes_each_first_runs_latest_prediction_made_before_it_started_against_the_rate_of_earlier_first_runs() {
    let directory = tempfile::tempdir().unwrap();
    let (_, db) = ledger(directory.path());
    let runs = [
        ("north#1", "2026-10-01T00:00:00Z", "2026-10-01T01:00:00Z", "done"),
        ("north#2", "2026-10-02T00:00:00Z", "2026-10-02T01:00:00Z", "not done"),
        ("north#3", "2026-10-03T00:00:00Z", "2026-10-03T01:00:00Z", "done"),
        ("north#4", "2026-10-04T00:00:00Z", "2026-10-04T01:00:00Z", "not done"),
        ("north#5", "2026-10-05T00:00:00Z", "2026-10-05T01:00:00Z", "done"),
    ];
    for (item, started, ended, outcome) in runs {
        db.execute(
            "INSERT INTO runs(agent, item, tier, category, started, ended, minutes, outcome, role) VALUES (?1, ?1, 'sonnet', 'tooling', ?2, ?3, 60, ?4, 'worker')",
            params![item, started, ended, outcome],
        )
        .unwrap();
    }
    let tooling = |item: &str, tier: &str, p: f64, made: &str| record(&db, item, "tooling", &[(tier.to_owned(), p)], at(made), Source::Dispatch).unwrap();
    tooling("north#2", "claude:sonnet-high", 0.2, "2026-10-01T23:00:00Z");
    tooling("north#3", "claude:sonnet-high", 0.5, "2026-10-02T12:00:00Z");
    tooling("north#3", "claude:sonnet-high", 0.9, "2026-10-02T23:00:00Z");
    tooling("north#4", "claude:sonnet-high", 0.6, "2026-10-03T23:00:00Z");
    tooling("north#4", "claude:sonnet-high", 0.1, "2026-10-04T00:30:00Z");
    tooling("north#5", "claude:opus-medium", 0.3, "2026-10-04T23:00:00Z");

    let scored = score_with(&db, &BTreeSet::new(), "2026-10-02T12:00:00Z").unwrap();
    let predicted = ((0.9_f64 - 1.0).powi(2) + 0.6_f64.powi(2)) / 2.0;
    let base = ((0.5_f64 - 1.0).powi(2) + (2.0_f64 / 3.0).powi(2)) / 2.0;
    assert_eq!(scored.len(), 1, "{scored:?}");
    assert_eq!((scored[0].category.as_str(), scored[0].n), ("tooling", 2), "north#2 ended before the window and north#5's prediction names another tier");
    assert!((scored[0].predicted - predicted).abs() < 1e-12 && (scored[0].base - base).abs() < 1e-12, "{scored:?}");
    assert_eq!(scored[0].line("7d"), "tier success tooling: Brier 0.185 against base-rate Brier 0.347 (n 2, 7d)");

    let failed = score_with(&db, &BTreeSet::from(["north#3".to_owned()]), "2026-10-02T12:00:00Z").unwrap();
    assert!((failed[0].predicted - (0.9_f64.powi(2) + 0.6_f64.powi(2)) / 2.0).abs() < 1e-12, "an escalated run scores as a failure: {failed:?}");
}

#[test]
fn a_ledger_with_no_stored_prediction_scores_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (_, db) = ledger(directory.path());
    assert_eq!(score_with(&db, &BTreeSet::new(), "2026-10-01T00:00:00Z").unwrap(), Vec::<Brier>::new());
}
