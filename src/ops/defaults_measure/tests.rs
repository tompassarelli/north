use std::path::Path;

use rusqlite::Connection;
use serde_json::Value;

use super::registry::Registry;
use super::{Args, Context, measure, parse, priors, rollouts, sweeps, training};
use crate::foundation::clock::Timestamp;

fn fixture(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE runs (
            agent TEXT PRIMARY KEY, item TEXT, follows TEXT, tier TEXT, category TEXT,
            started TEXT, ended TEXT, minutes INTEGER, eta_min INTEGER, outcome TEXT,
            spec TEXT, scope TEXT, surface TEXT, verify TEXT, role TEXT,
            expects_landing INTEGER, landed INTEGER, landed_at TEXT, cost_usd REAL);
         CREATE TABLE calibration (at TEXT, value REAL);
         CREATE TABLE usage (ts TEXT, used_pct REAL);",
    )
    .unwrap();
    let start = Timestamp::parse("2026-09-28T00:00:00Z").unwrap().unix_seconds();
    for i in 0..120i64 {
        let at = start + i * 3 * 3600;
        let iso = |seconds: i64| Timestamp::from_unix_seconds(seconds).iso();
        let surface = if i % 3 != 0 { "shell" } else { "api" };
        let held = i == 45 || i == 85;
        db.execute(
            "INSERT INTO runs VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                format!("synthetic-{i}"),
                "fixture#1",
                None::<String>,
                "codex:sol-high",
                "tooling",
                iso(at),
                iso(at + 1800),
                5 + i % 7 * 5,
                20,
                "done",
                "measured",
                "module",
                surface,
                "local-test",
                "worker",
                i64::from(held),
                i64::from(!held),
                None::<String>,
                0.1 + (i % 3) as f64
            ],
        )
        .unwrap();
    }
}

fn context(registry: Registry, since: &str) -> Context {
    Context { registry, state: Some("/nonexistent".into()), codex_root: "/nonexistent".into(), days: 7.0, since: since.to_owned() }
}

fn propose(path: &Path, registry: Registry, kind: Option<&str>) -> String {
    let arguments: Vec<String> = [path.to_str().unwrap()]
        .into_iter()
        .chain(kind)
        .chain(["--propose", "--train-until", "2026-10-12", "--since", "2026-10-05", "--json"])
        .map(str::to_owned)
        .collect();
    let args = parse(&arguments).unwrap();
    let mut out = Vec::new();
    measure(&args, &context(registry, &args.since), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn rows(text: &str) -> Vec<Value> {
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn printed(value: &Value) -> Value {
    serde_json::from_str(&value.to_string()).unwrap()
}

fn row<'a>(rows: &'a [Value], name: &str) -> &'a Value {
    rows.iter().find(|row| row["default"] == name).unwrap()
}

#[test]
fn held_out_evidence_cannot_change_proposals_and_the_ledger_copy_stays_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("training.db");
    fixture(&path);
    let before = std::fs::read(&path).unwrap();
    let first = propose(&path, Registry::bundled(), None);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(propose(&path, Registry::bundled(), None), first);
    let proposals = rows(&first);
    let registry = Registry::bundled();
    let measured: Vec<&str> = registry.entries().iter().filter(|entry| entry.source.starts_with("measured:")).map(|entry| entry.name.as_str()).collect();
    assert_eq!(proposals.len(), measured.len());
    for proposal in &proposals {
        let name = proposal["default"].as_str().unwrap();
        assert!(measured.contains(&name));
        assert_eq!(&proposal["current"], registry.value(name));
        for key in ["report", "proposed", "n", "score_current", "score_proposed", "train_until", "since", "objective", "direction", "eligible", "reason"] {
            assert!(proposal.get(key).is_some(), "{name} lacks {key}");
        }
    }
    let threshold = row(&proposals, "success_threshold");
    assert_eq!(threshold["eligible"], false);
    assert!(threshold["reason"].as_str().unwrap().contains("production replay required"));
    assert!(threshold["reason"].as_str().unwrap().contains("untimestamped costs unavailable"));
    assert_eq!(row(&proposals, "eta_min_samples")["eligible"], true);
    let gap = row(&proposals, "box_chain_gap_min");
    assert!(gap["score_current"].is_null());
    assert_eq!(gap["proposed"], gap["current"]);

    let cutoff = training::iso_stamp("2026-10-12").unwrap();
    let since = training::iso_stamp("2026-10-05").unwrap();
    let training = training::training_db(&path, &cutoff).unwrap();
    for (name, field) in [("success_threshold", "rate"), ("eta_min_samples", "median_abs_log_err"), ("eta_coverage", "cost")] {
        let current = registry.value(name).as_f64().unwrap();
        let scores = match name {
            "success_threshold" => sweeps::sweep_tier(&training, &registry, &since, name, None),
            "eta_min_samples" => sweeps::sweep_eta_min_samples(&training, &registry, &since, None),
            _ => sweeps::sweep_eta_coverage(&training, &registry, &since, None),
        }
        .unwrap();
        let baseline = scores.iter().find(|score| score.get("value").as_f64() == Some(current)).unwrap();
        assert_eq!(row(&proposals, name)["score_current"], printed(baseline.get(field)), "{name}");
    }
    assert!(training.execute("DELETE FROM runs", []).is_err());
    drop(training);

    let db = Connection::open(&path).unwrap();
    for (i, at) in ["2026-10-12T00:00:00Z", "2026-10-12T05:00:00+05:00", "2026-10-12T00:00:00.000001Z", "2026-10-14T00:00:00Z"].iter().enumerate() {
        db.execute(
            "INSERT INTO runs SELECT ?1, item, ?2, tier, category, ?3, ?3, 10000, eta_min, 'not done',
             spec, scope, surface, verify, role, 0, 0, NULL, 1000 FROM runs WHERE agent = 'synthetic-50'",
            rusqlite::params![format!("future-{i}"), if i % 2 == 0 { None } else { Some("synthetic-70") }, at],
        )
        .unwrap();
    }
    db.execute_batch(
        "UPDATE runs SET landed = 1, landed_at = '2026-10-12T00:00:00Z' WHERE agent = 'synthetic-45';
         UPDATE runs SET landed = 1, landed_at = NULL WHERE agent = 'synthetic-85';
         UPDATE runs SET cost_usd = 999;
         INSERT INTO calibration VALUES ('2026-10-12T00:00:00Z', 999);
         INSERT INTO usage VALUES ('2026-10-12T00:00:00Z', 100);",
    )
    .unwrap();
    drop(db);
    let held_out = std::fs::read(&path).unwrap();
    assert_eq!(propose(&path, Registry::bundled(), None), first, "held-out, Follows or cost evidence leaked into training");
    assert_eq!(std::fs::read(&path).unwrap(), held_out);
}

#[test]
fn an_off_grid_current_value_is_scored_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("training.db");
    fixture(&path);
    let bundled = include_str!("../../../harness/defaults.toml");
    let current = Registry::bundled().value("eta_min_samples").as_i64().unwrap();
    let off_grid = (1..20).find(|value| ![1, 2, 3, 5, 8, current].contains(value)).unwrap();
    let original = format!("[eta_min_samples]\nvalue = {current}");
    assert_eq!(bundled.matches(&original).count(), 1);
    let registry = || Registry::parse(&bundled.replace(&original, &format!("[eta_min_samples]\nvalue = {off_grid}"))).unwrap();
    let proposal = rows(&propose(&path, registry(), Some("eta_min_samples")));
    assert_eq!(proposal.len(), 1);
    assert_eq!(proposal[0]["current"], off_grid);
    let training = training::training_db(&path, &training::iso_stamp("2026-10-12").unwrap()).unwrap();
    let since = training::iso_stamp("2026-10-05").unwrap();
    let exact = sweeps::sweep_eta_min_samples(&training, &registry(), &since, Some(&[off_grid as f64])).unwrap();
    assert_eq!(proposal[0]["score_current"], printed(exact[0].get("median_abs_log_err")));
}

#[test]
fn an_empty_ledger_proposes_every_current_value_unscored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.db");
    fixture(&path);
    Connection::open(&path).unwrap().execute("DELETE FROM runs", []).unwrap();
    for proposal in rows(&propose(&path, Registry::bundled(), None)) {
        assert!(proposal["score_current"].is_null());
        assert!(proposal["score_proposed"].is_null());
        assert_eq!(proposal["current"], proposal["proposed"]);
        assert_eq!(proposal["eligible"], false);
    }
}

#[test]
fn invalid_proposal_flags_are_refused() {
    for flags in [
        &["db", "--propose", "--json"][..],
        &["db", "--propose", "--train-until", "invalid", "--json"],
        &["db", "--propose", "--train-until", "2026-10-01", "--since", "2026-10-05", "--json"],
        &["db", "--train-until", "2026-10-12"],
        &["db", "--picks", "--json"],
        &["db"],
        &["db", "no_such_kind"],
    ] {
        let arguments: Vec<String> = flags.iter().map(|flag| (*flag).to_owned()).collect();
        assert!(parse(&arguments).is_err(), "{flags:?}");
    }
    let accepted: Args = parse(&["--priors".to_owned(), "db".to_owned()]).unwrap();
    assert_eq!(accepted.priors.as_deref(), Some(Path::new("db")));
}

#[test]
fn every_prior_reports_a_count_against_its_need() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.db");
    fixture(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP TABLE usage; CREATE TABLE usage (ts, provider, account, kind, name, used_pct, resets_at);
             CREATE TABLE model_events (model, kind, at);",
        )
        .unwrap();
    let registry = Registry::bundled();
    let lines = priors::priors(&training::open_read_only(&path).unwrap(), &context(Registry::bundled(), "2026-10-05")).unwrap();
    let names: Vec<&str> = registry.entries().iter().filter(|entry| entry.source.starts_with("prior: ")).map(|entry| entry.name.as_str()).collect();
    assert_eq!(lines.len(), names.len());
    for (line, name) in lines.iter().zip(&names) {
        let words: Vec<&str> = line.split(' ').collect();
        assert!(matches!(words[0], "SETTLED" | "WAIT"), "{line}");
        assert_eq!(words[1], *name);
        assert!(words[2].strip_prefix("n=").is_some_and(|n| n.split('/').next().unwrap().parse::<usize>().is_ok()), "{line}");
    }
    assert!(lines.iter().any(|line| line.contains("n=120")), "count_worker_runs counts every judged first run: {lines:?}");
}

#[test]
fn a_capacity_episode_runs_from_the_first_capacity_turn_to_the_next_ok_turn() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions/2026/10/01");
    std::fs::create_dir_all(&sessions).unwrap();
    let turn = |at: &str, error: &str| format!(r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"task_complete"{error}}}}}"#);
    let capacity = r#","error":{"codex_error_info":"server_overloaded"}"#;
    let lines = [
        r#"{"timestamp":"2026-10-01T00:00:00Z","type":"turn_context","payload":{"model":"gpt-6.1-sol"}}"#.to_owned(),
        turn("2026-10-01T00:00:00Z", capacity),
        turn("2026-10-01T00:05:00Z", capacity),
        turn("2026-10-01T00:12:00Z", ""),
        turn("2026-10-01T00:20:00Z", capacity),
    ];
    std::fs::write(sessions.join("rollout-a.jsonl"), lines.join("\n")).unwrap();
    let episodes = rollouts::capacity_episodes(&rollouts::turns(dir.path(), 0.0));
    assert_eq!((episodes.closed, episodes.p90), (1, Some(12.0)));
    assert_eq!(episodes.rows[0].get("capacity_turns"), &Value::from(3));
    assert_eq!(episodes.rows[0].get("unclosed"), &Value::from(1));
}

