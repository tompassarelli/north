use std::path::Path;

use rusqlite::{Connection, params};

use super::{Band, asked, cuts, items, recommend};

struct Shape {
    category: &'static str,
    scope: &'static str,
    surface: &'static str,
    count: usize,
    /// Each run: tier, minutes, outcome, report.
    runs: &'static [(&'static str, i64, &'static str, &'static str)],
}

const EASY: Shape = Shape { category: "mechanical", scope: "one-file", surface: "shell", count: 40, runs: &[("medium", 10, "done", "Done:")] };
const MIDDLE: Shape = Shape {
    category: "feature",
    scope: "module",
    surface: "rust",
    count: 12,
    runs: &[("medium", 60, "not done", "Not done: Stop: reasoning."), ("high", 60, "done", "Done:")],
};
const HARD: Shape = Shape {
    category: "netcode-determinism",
    scope: "cross-module",
    surface: "ts",
    count: 9,
    runs: &[
        ("medium", 120, "not done", "Not done: Stop: reasoning."),
        ("high", 120, "not done", "Not done: Stop: decision."),
        ("xhigh", 360, "done", "Done:"),
    ],
};
const ETA: i64 = 60;

fn item_ref(shape: &Shape, index: usize) -> String {
    format!("{}#{index}", shape.category)
}

fn stamp(minute: i64) -> String {
    format!("2026-10-01T{:02}:{:02}:00Z", minute / 60, minute % 60)
}

fn ledger(path: &Path, withheld: Option<&str>) {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE runs(agent TEXT PRIMARY KEY, item TEXT, follows TEXT, tier TEXT, category TEXT, scope TEXT, surface TEXT,
         staffed TEXT, started TEXT, ended TEXT, minutes INTEGER, eta_min INTEGER, outcome TEXT, report TEXT, arm TEXT,
         compactions INTEGER, brief_text TEXT, effort TEXT)",
    )
    .unwrap();
    for shape in [&EASY, &MIDDLE, &HARD] {
        for index in 0..shape.count {
            let item = item_ref(shape, index);
            if withheld == Some(item.as_str()) {
                continue;
            }
            let (mut minute, mut follows) = (0, None);
            for (step, (tier, minutes, outcome, report)) in shape.runs.iter().enumerate() {
                let agent = format!("{item}-{step}");
                let eta = if shape.category == EASY.category { 10 } else { ETA };
                db.execute(
                    "INSERT INTO runs(agent, item, follows, tier, category, scope, surface, started, ended, minutes, eta_min, outcome, report)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![agent, item, follows, tier, shape.category, shape.scope, shape.surface, stamp(minute), stamp(minute + minutes), minutes, eta, outcome, report],
                )
                .unwrap();
                minute += minutes;
                follows = Some(agent);
            }
        }
    }
    for (index, effort) in ["high", "high", "high", "xhigh"].into_iter().enumerate() {
        db.execute(
            "INSERT INTO runs(agent, tier, ended, minutes, outcome, report, brief_text, effort) VALUES(?1, 'high', ?2, 5, 'done', 'Done:', ?3, ?4)",
            params![format!("plan-{index}"), stamp(0), "Item: north#1\nTier: planner\nCategory: feature", effort],
        )
        .unwrap();
    }
}

fn facets(shape: &Shape) -> String {
    let eta = if shape.category == EASY.category { 10 } else { ETA };
    format!("Category: {}, Scope: {}, Surface: {}, Summed box ETA: {eta}m, Prior Not dones: 0", shape.category, shape.scope, shape.surface)
}

#[test]
fn five_held_out_items_print_their_realized_band() {
    let directory = tempfile::tempdir().unwrap();
    let full = directory.path().join("full.db");
    ledger(&full, None);
    let all = items(&Connection::open(&full).unwrap()).unwrap();
    let held_out = [(&EASY, 3), (&EASY, 17), (&MIDDLE, 5), (&MIDDLE, 9), (&HARD, 2)];
    let mut bands = Vec::new();
    for (shape, index) in held_out {
        let item = item_ref(shape, index);
        let training_db = directory.path().join(format!("{item}.db"));
        ledger(&training_db, Some(&item));
        let training = items(&Connection::open(&training_db).unwrap()).unwrap();
        let realized = cuts(&training).band(all.iter().find(|past| past.item == item).unwrap().difficulty);
        let printed = recommend(&training_db, &facets(shape)).unwrap();
        let band = printed.split_whitespace().nth(1).unwrap().to_owned();
        assert_eq!(band, realized.name(), "{item}: printed {printed}");
        bands.push(realized);
    }
    assert_eq!(bands, [Band::High, Band::High, Band::Xhigh, Band::Xhigh, Band::Max], "the held-out items span every band");
}

#[test]
fn cut_points_come_from_where_planning_paid_off_not_a_fixed_split() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    ledger(&path, None);
    let past = items(&Connection::open(&path).unwrap()).unwrap();
    let measured = cuts(&past);
    for item in &past {
        let earned = [Band::High, Band::Xhigh, Band::Max][item.benefit];
        assert_eq!(measured.band(item.difficulty), earned, "{}", item.item);
    }
    let printed = recommend(&path, &facets(&MIDDLE)).unwrap();
    assert!(printed.contains("realized items high/xhigh/max=65.57/19.67/14.75% of 61\n"), "{printed}");
}

#[test]
fn prints_the_realized_share_of_planner_runs_at_each_effort() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    ledger(&path, None);
    let printed = recommend(&path, &facets(&EASY)).unwrap();
    assert!(printed.starts_with("planner high  nearest items n=8  "), "{printed}");
    assert!(printed.contains("planner runs by effort n=4: high=3 (75.00%) xhigh=1 (25.00%) max=0 (0.00%)\n"), "{printed}");
}

#[test]
fn a_brief_missing_a_planning_facet_is_refused() {
    assert!(asked("Category: feature, Scope: module, Surface: rust, Summed box ETA: 60m, Prior Not dones: 1").is_ok());
    for refused in [
        "Category: feature, Scope: module, Summed box ETA: 60m, Prior Not dones: 1",
        "Category: feature, Scope: module, Surface: rust, Summed box ETA: 0, Prior Not dones: 0",
        "Category: feature, Scope: modul, Surface: rust, Summed box ETA: 9, Prior Not dones: 0",
        "Category: sorcery, Scope: module, Surface: rust, Summed box ETA: 9, Prior Not dones: 0",
    ] {
        assert!(asked(refused).is_err(), "{refused}");
    }
}

#[test]
fn an_item_with_a_run_still_going_is_left_out_of_the_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("threads.db");
    ledger(&path, None);
    let db = Connection::open(&path).unwrap();
    db.execute("INSERT INTO runs(agent, item, tier, started, ended, minutes, eta_min, outcome, report) VALUES('live', 'feature#0', 'high', ?1, '', 0, 60, 'running', '')", [stamp(0)]).unwrap();
    let past = items(&db).unwrap();
    assert_eq!(past.len(), 60);
    assert!(past.iter().all(|item| item.item != "feature#0"));
    assert!(recommend(&path, &facets(&MIDDLE)).unwrap().contains("of 60\n"));
}
