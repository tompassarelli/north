use std::collections::BTreeMap;

use rusqlite::Connection;

use super::boxes::{BoxRun, box_chains, box_duration};
use super::forward::{calibrate_box, calibrate_eta_pred, walk_forward_eta};
use super::recommend::{asked_of, recommend, recommend_eta, recommend_usd};
use super::verbs::since_start;
use super::{Knobs, Run, median, quantile, scaled, tier_key, tier_rank};
use crate::foundation::clock::Timestamp;
use crate::threads::runs::brief::Fields;

fn run(tier: &str, ok: bool, cost: Option<f64>) -> Run {
    Run { cells: ["research", "measured", "-", "-", "-"].map(str::to_owned), tier: tier.to_owned(), ok, cost_usd: cost, outcome: if ok { "done" } else { "not done" }.to_owned(), ..Run::default() }
}

fn many(count: usize, tier: &str, wins: usize, cost: f64) -> Vec<Run> {
    (0..count).map(|index| run(tier, index < wins, Some(cost))).collect()
}

fn asked(brief: &str) -> [Option<String>; 5] {
    asked_of(&Fields::read(brief))
}

fn pick(runs: &[Run], brief: &str, incumbent: &[(&str, &str)], priced: bool) -> (String, usize, Option<String>, String) {
    let held: BTreeMap<String, String> = incumbent.iter().map(|(cell, tier)| ((*cell).to_owned(), (*tier).to_owned())).collect();
    let found = recommend(&Knobs::registry(), runs, &asked(brief), &held, priced);
    (found.cell, found.n, found.tier, found.rule)
}

#[test]
fn quantile_takes_the_ceiling_rank_and_median_the_lower_middle() {
    assert_eq!(quantile(&[40.0, 10.0, 30.0, 20.0], 0.75), Some(30.0));
    assert_eq!(quantile(&[10.0, 20.0, 30.0, 40.0, 50.0], 0.5), Some(30.0));
    assert_eq!(quantile(&[], 0.5), None);
    assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.0));
    assert_eq!(median(&[]), None);
    assert_eq!(scaled(4.2, Some(1.5), true), 7.0);
    assert_eq!(scaled(4.0, None, false), 4.0);
}

#[test]
fn a_run_tier_maps_to_its_key_and_orders_by_rank() {
    assert_eq!(tier_key("medium"), "claude:opus-medium");
    assert_eq!(tier_key(" claude:haiku "), "claude:haiku");
    assert_eq!(tier_key(""), "-");
    assert!(tier_rank("haiku") < tier_rank("gpt-6.1-sol high"));
    assert_eq!(tier_rank("mystery"), 10);
}

#[test]
fn recommend_backs_off_to_the_finest_cell_with_min_runs_and_names_it() {
    let mut runs = many(5, "claude:haiku", 5, 1.0);
    runs.extend(many(2, "claude:opus-medium", 2, 1.0));
    runs[5].cells[2] = "one-file".to_owned();
    runs[6].cells[2] = "one-file".to_owned();
    let (cell, n, tier, rule) = pick(&runs, "Category: research, Spec: measured, Scope: one-file", &[], true);
    assert_eq!((cell.as_str(), n, tier.as_deref(), rule.as_str()), ("Category: research, Spec: measured", 7, Some("claude:haiku"), "rank (one tier passes)"));
}

#[test]
fn recommend_prefers_the_cheapest_dollars_per_success_when_every_passing_tier_is_priced() {
    let mut runs = many(5, "claude:haiku", 5, 1.0);
    runs.extend(many(5, "codex:sol-medium", 5, 0.5));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], true);
    assert_eq!((tier.as_deref(), rule.as_str()), (Some("codex:sol-medium"), "$ per success"));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], false);
    assert_eq!((tier.as_deref(), rule.as_str()), (Some("claude:haiku"), "rank"));
}

#[test]
fn recommend_falls_back_to_rank_and_says_which_tiers_have_thin_dollars() {
    let mut runs = many(5, "claude:haiku", 5, 1.0);
    runs.extend((0..5).map(|_| run("codex:sol-medium", true, None)));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], true);
    assert_eq!((tier.as_deref(), rule.as_str()), (Some("claude:haiku"), "rank (uncalibrated: thin $ codex:sol-medium n=0 < 5)"));
}

#[test]
fn recommend_keeps_the_incumbent_unless_a_dearer_challenger_beats_it_by_the_margin() {
    let mut runs = many(10, "claude:haiku", 7, 1.0);
    runs.extend(many(5, "claude:opus-medium", 4, 1.0));
    let (cell, _, tier, _) = pick(&runs, "Category: research", &[], true);
    assert_eq!(tier.as_deref(), Some("claude:opus-medium"));
    let (_, _, held, _) = pick(&runs, "Category: research", &[(&cell, "haiku")], true);
    assert_eq!(held.as_deref(), Some("claude:haiku"));
    let mut sure = many(10, "claude:opus-medium", 10, 1.0);
    sure.extend(many(10, "claude:haiku", 7, 1.0));
    let (_, _, moved, _) = pick(&sure, "Category: research", &[(&cell, "claude:haiku")], true);
    assert_eq!(moved.as_deref(), Some("claude:opus-medium"));
}

#[test]
fn recommend_with_no_passing_tier_takes_the_lowest_measured_cost_or_admits_it_has_none() {
    with_no_passing_tier_the_pick_is_the_lowest_measured_cost_among_sampled_tiers_for_any_run_mix();
    let mut runs = many(5, "claude:haiku", 1, 1.0);
    runs.extend(many(5, "codex:sol-medium", 2, 1.0));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], true);
    assert_eq!(tier.as_deref(), Some("codex:sol-medium"));
    assert!(rule.starts_with("$ per success (no tier passes"));
    let unpriced: Vec<Run> = (0..5).map(|_| run("claude:haiku", false, None)).collect();
    let (_, _, tier, rule) = pick(&unpriced, "Category: research", &[], true);
    assert_eq!(tier, None);
    assert!(rule.starts_with("insufficient evidence"));
}

#[test]
fn a_knob_override_moves_the_pick_without_touching_the_registry() {
    let runs = many(3, "claude:haiku", 3, 1.0);
    let held = BTreeMap::new();
    let strict = recommend(&Knobs::registry(), &runs, &asked("Category: research"), &held, true);
    let relaxed = recommend(&Knobs { min_runs: 3, usd_min: 3, ..Knobs::registry() }, &runs, &asked("Category: research"), &held, true);
    assert_eq!((strict.tier, relaxed.tier.as_deref()), (None, Some("claude:haiku")));
}

fn timed(minutes: &[f64]) -> Vec<Run> {
    minutes
        .iter()
        .enumerate()
        .map(|(index, minutes)| Run { cells: ["mechanical", "-", "-", "-", "-"].map(str::to_owned), tier: "medium".into(), minutes: Some(*minutes), eta_min: Some(30.0), first: true, ended: format!("2026-10-0{}T00:00:00Z", index + 1), ..Run::default() })
        .collect()
}

#[test]
fn walk_forward_eta_predicts_each_run_from_strictly_earlier_runs_of_its_tier() {
    let runs = timed(&[10.0, 20.0, 30.0, 40.0, 50.0]);
    let predicted: Vec<(f64, f64)> = walk_forward_eta(&Knobs::registry(), &runs, "", "9999").into_iter().map(|(run, eta)| (run.minutes.unwrap(), eta)).collect();
    assert_eq!(predicted, [(20.0, 10.0), (30.0, 20.0), (40.0, 30.0), (50.0, 30.0)]);
    let window: Vec<f64> = walk_forward_eta(&Knobs::registry(), &runs, "2026-10-04T00:00:00Z", "2026-10-05T00:00:00Z").into_iter().map(|(run, _)| run.minutes.unwrap()).collect();
    assert_eq!(window, [40.0]);
}

#[test]
fn eta_prediction_backs_off_a_facet_until_enough_samples_and_ignores_other_tiers() {
    eta_p75_takes_the_third_of_three_durations_and_backs_off_the_two_run_scope_cell();
    let mut runs = timed(&[10.0, 20.0, 30.0, 99.0]);
    runs[3].tier = "claude:haiku".into();
    runs[0].cells[1] = "measured".into();
    let refs: Vec<&Run> = runs.iter().collect();
    let knobs = Knobs::registry();
    assert_eq!(recommend_eta(&knobs, &refs, "Category: mechanical, Spec: measured", "claude:opus-medium"), (Some(30.0), 3, "Category: mechanical".to_owned()));
    assert_eq!(recommend_eta(&knobs, &refs, "Category: mechanical", "haiku"), (Some(99.0), 1, "Category: mechanical".to_owned()));
}

#[test]
fn the_eta_prediction_ratio_row_gives_the_scale_covering_the_target_share_of_runs() {
    calibrating_the_walk_forward_eta_brings_its_coverage_closer_to_the_target_on_a_drifting_ledger();
    let runs = timed(&[10.0, 20.0, 30.0, 40.0, 50.0]);
    let all = calibrate_eta_pred(&Knobs::registry(), &runs, "", "9999").into_iter().find(|row| row.cell == "all").unwrap();
    assert_eq!((all.n, all.p25, all.median, all.p75), (4, Some(4.0 / 3.0), Some(1.5), Some(5.0 / 3.0)));
    assert_eq!(all.value.map(super::forward::Figure::real), Some(5.0 / 3.0));
}

#[test]
fn dollars_per_done_run_back_off_from_the_category_and_tier_to_the_tier() {
    let mut runs: Vec<Run> = (1..=3).map(|cost| Run { outcome: "done".into(), cost_usd: Some(f64::from(cost)), tier: "medium".into(), cells: ["tooling", "-", "-", "-", "-"].map(str::to_owned), ..Run::default() }).collect();
    runs.push(Run { outcome: "done".into(), cost_usd: Some(9.0), tier: "medium".into(), cells: ["research", "-", "-", "-", "-"].map(str::to_owned), ..Run::default() });
    let refs: Vec<&Run> = runs.iter().collect();
    assert_eq!(recommend_usd(&refs, "tooling", "medium"), (Some(2.0), 3));
    assert_eq!(recommend_usd(&refs, "research", "medium"), (Some(2.0), 4));
    assert_eq!(recommend_usd(&refs, "research", "haiku"), (None, 0));
}

fn landed(item: &str, staffed: &str, ended: &str, landed_at: Option<&str>) -> BoxRun {
    BoxRun { item: item.into(), category: "feature".into(), outcome: "done".into(), staffed: Some(staffed.into()), ended: ended.into(), landed: landed_at.is_some(), landed_at: landed_at.map(str::to_owned), ..BoxRun::default() }
}

#[test]
fn box_chains_join_runs_staffed_within_the_gap_and_count_only_landed_boxes() {
    let rows = vec![
        landed("north#1", "2026-10-01T00:00:00Z", "2026-10-01T00:10:00Z", None),
        landed("north#1", "2026-10-01T00:20:00Z", "2026-10-01T00:50:00Z", Some("2026-10-01T00:55:00Z")),
        landed("north#1", "2026-10-01T03:00:00Z", "2026-10-01T03:10:00Z", Some("2026-10-01T03:10:00Z")),
        landed("north#2", "2026-10-01T00:00:00Z", "2026-10-01T00:05:00Z", None),
    ];
    let knobs = Knobs::registry();
    let chains = box_chains(&knobs, &rows);
    assert_eq!(chains.iter().map(|chain| (chain.item.as_str(), chain.runs, chain.minutes)).collect::<Vec<_>>(), [("north#1", 2, 55.0), ("north#1", 1, 10.0)]);
    assert_eq!(chains[0].category.as_deref(), Some("feature"));
    let wider = box_chains(&Knobs { box_chain_gap_min: 180.0, ..knobs.clone() }, &rows);
    assert_eq!(wider.iter().map(|chain| chain.minutes).collect::<Vec<_>>(), [190.0]);
    assert_eq!(box_duration(&knobs, &chains, Some("feature")), (Some(55.0), 2, "box p75 all categories".to_owned()));
    let enough = Knobs { eta_min_samples: 2, ..knobs };
    assert_eq!(box_duration(&enough, &chains, Some("feature")), (Some(55.0), 2, "box p75 Category: feature".to_owned()));
}

#[test]
fn box_calibration_predicts_each_chain_only_from_chains_landed_before_it_was_staffed() {
    let rows = vec![
        landed("north#1", "2026-10-01T00:00:00Z", "2026-10-01T00:10:00Z", Some("2026-10-01T00:10:00Z")),
        landed("north#2", "2026-10-02T00:00:00Z", "2026-10-02T00:20:00Z", Some("2026-10-02T00:20:00Z")),
    ];
    let knobs = Knobs::registry();
    let rows = calibrate_box(&knobs, &box_chains(&knobs, &rows), "2026-10-01T00:00:00Z", "2026-10-03T00:00:00Z");
    assert_eq!((rows[0].cell.as_str(), rows[0].n, rows[0].median), ("all", 1, Some(2.0)));
}

#[test]
fn recommend_verb_prints_the_pick_the_stats_and_the_eta_and_remembers_the_choice() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("threads.db");
    let db = Connection::open(&database).unwrap();
    db.execute_batch(include_str!("../runs/fixtures/ledger.sql")).unwrap();
    db.execute_batch("CREATE TABLE calibration(at TEXT NOT NULL, kind TEXT NOT NULL, cell TEXT NOT NULL, n INTEGER NOT NULL, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT NOT NULL) STRICT; CREATE TABLE tier_choice(cell TEXT PRIMARY KEY, tier TEXT NOT NULL, since TEXT NOT NULL) STRICT;").unwrap();
    for index in 0..5 {
        db.execute(
            "INSERT INTO runs(agent, tier, category, spec, ended, minutes, outcome, cost_usd, role) VALUES (?1, 'claude:haiku', 'research', 'measured', ?2, ?3, 'done', 0.5, 'worker')",
            rusqlite::params![format!("a{index}"), format!("2026-10-0{}T00:00:00Z", index + 1), 10 * (index + 1)],
        )
        .unwrap();
    }
    let configuration = directory.path().join("orchestration.toml");
    std::fs::write(&configuration, "[tiers.\"claude:haiku\"]\nmodel = \"haiku\"\neffort = \"low\"\nstart = 0\n").unwrap();
    let outside = crate::threads::fake::Recorded::default();
    let context = crate::threads::verb::Context { database: &database, actor: "tester".into(), no_native: None, outside: &outside, now: crate::foundation::clock::Timestamp::parse("2026-10-10T00:00:00Z").unwrap(), replica: false };
    let args = crate::threads::verb::Args::parse(&[], &["--since"], &["--planner"]).ok().unwrap();
    let fields = Fields::read("Category: research, Spec: measured");
    let text = match super::verbs::recommend_with(&args, &context, &fields, &configuration) {
        Ok(text) => text,
        Err(_) => panic!("recommend refused"),
    };
    assert_eq!(
        text,
        "tier claude:haiku  cell [Category: research, Spec: measured]  n 5\n\
         success threshold 80%  rule rank (one tier passes)\n\
         \x20 claude:haiku       100% of 5  $0.50/success (priced n=5)\n\
         eta 40m (p75 of n=5, cell Category: research, Spec: measured, tier claude:haiku, uncalibrated: no eta_pred calibration)  $0.50 (median of n=5, uncalibrated: no usd calibration)\n"
    );
    let held: String = db.query_row("SELECT tier FROM tier_choice WHERE cell = 'Category: research, Spec: measured'", [], |row| row.get(0)).unwrap();
    assert_eq!(held, "claude:haiku");
}

fn fixture_ledger(directory: &std::path::Path, extra: &str) -> std::path::PathBuf {
    let database = directory.join("threads.db");
    let db = Connection::open(&database).unwrap();
    db.execute_batch(include_str!("../runs/fixtures/ledger.sql")).unwrap();
    db.execute_batch(include_str!("fixtures/summary.sql")).unwrap();
    db.execute_batch(extra).unwrap();
    database
}

#[test]
fn summary_equals_the_python_output_on_one_ledger_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let database = fixture_ledger(directory.path(), "");
    let outside = crate::threads::fake::Recorded::default();
    let context = crate::threads::verb::Context { database: &database, actor: "tester".into(), no_native: None, outside: &outside, now: crate::foundation::clock::Timestamp::parse("2026-10-12T00:00:00Z").unwrap(), replica: false };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = super::summary::run(&[], &context, &mut out, &mut err);
    assert_eq!((code, String::from_utf8(err).unwrap()), (0, String::new()));
    assert_eq!(String::from_utf8(out).unwrap(), include_str!("fixtures/summary.txt"));
}

#[test]
fn routing_compare_equals_the_python_output_on_one_ledger_with_a_costs_file() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("threads.db");
    let db = Connection::open(&database).unwrap();
    db.execute_batch(include_str!("../runs/fixtures/ledger.sql")).unwrap();
    db.execute_batch(include_str!("fixtures/routing.sql")).unwrap();
    drop(db);
    let outside = crate::threads::fake::Recorded { prices: vec![("claude-b", 50.0), ("codex", 100.0)], ..Default::default() };
    let context = crate::threads::verb::Context { database: &database, actor: "tester".into(), no_native: None, outside: &outside, now: crate::foundation::clock::Timestamp::parse("2026-10-29T00:00:00Z").unwrap(), replica: false };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = super::routing::run(&context, &mut out, &mut err);
    assert_eq!((code, String::from_utf8(err).unwrap()), (0, String::new()));
    let expected = include_str!("fixtures/routing.txt").replace("{ledger}", &std::fs::canonicalize(&database).unwrap().display().to_string());
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn the_bootstrap_draws_python_random_seeded_zero() {
    let mut twister = crate::platform::twister::Twister::seeded(0);
    assert_eq!(twister.choices(&[1.0, 2.0, 3.0, 4.0, 5.0], 8), [5.0, 4.0, 3.0, 2.0, 3.0, 3.0, 4.0, 2.0]);
}

struct Draws(u64);

impl Draws {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() * bound as f64) as usize
    }

    fn gauss(&mut self) -> f64 {
        (-2.0 * (1.0 - self.next()).ln()).sqrt() * (std::f64::consts::TAU * self.next()).cos()
    }
}

fn eta_p75_takes_the_third_of_three_durations_and_backs_off_the_two_run_scope_cell() {
    let mut runs: Vec<Run> = [5.0, 5.0, 20.0].iter().map(|minutes| Run { cells: ["research", "measured", "-", "-", "-"].map(str::to_owned), tier: "medium".into(), minutes: Some(*minutes), ..Run::default() }).collect();
    runs[0].cells[2] = "one-file".into();
    runs[1].cells[2] = "one-file".into();
    let refs: Vec<&Run> = runs.iter().collect();
    let knobs = Knobs::registry();
    for cell in ["Category: research, Spec: measured", "Category: research, Spec: measured, Scope: one-file"] {
        assert_eq!(recommend_eta(&knobs, &refs, cell, "medium"), (Some(20.0), 3, "Category: research, Spec: measured".to_owned()), "{cell}");
    }
}

fn with_no_passing_tier_the_pick_is_the_lowest_measured_cost_among_sampled_tiers_for_any_run_mix() {
    let knobs = Knobs::registry();
    let tiers = ["codex:sol-medium", "codex:sol-high", "codex:sol-xhigh"];
    let mut draws = Draws(131_013);
    for case in 0..240 {
        let mut runs = Vec::new();
        for tier in tiers {
            let n = draws.below(21);
            let wins = draws.below((n * knobs.success_threshold as usize).div_ceil(100).max(1));
            runs.extend((0..n).map(|index| run(tier, index < wins, (draws.next() >= 0.35).then(|| (1 + draws.below(99)) as f64 / 100.0))));
        }
        let asked = asked("Category: research, Spec: measured");
        let found = recommend(&knobs, &runs, &asked, &BTreeMap::new(), true);
        let expected = found.stats.iter().filter(|(_, stat)| stat.n >= knobs.min_runs && stat.usd.is_some()).min_by(|(x, a), (y, b)| a.usd.partial_cmp(&b.usd).unwrap().then(tier_rank(x).cmp(&tier_rank(y)))).map(|(tier, _)| tier.clone());
        assert_eq!(found.tier, expected, "case {case}");
        if let Some(tier) = &found.tier {
            let stat = &found.stats.iter().find(|(key, _)| key == tier).unwrap().1;
            assert!(stat.n >= knobs.min_runs && stat.usd_n >= knobs.usd_min, "case {case}");
        }
        let held = BTreeMap::from([(found.cell.clone(), "codex:sol-xhigh".to_owned())]);
        assert_eq!(recommend(&knobs, &runs, &asked, &held, true).tier, expected, "case {case} held");
    }
}

fn calibrating_the_walk_forward_eta_brings_its_coverage_closer_to_the_target_on_a_drifting_ledger() {
    let knobs = Knobs::registry();
    let mut draws = Draws(35);
    let end = 1_790_000_000_i64;
    let runs: Vec<Run> = (0..720)
        .map(|index| {
            let progress = index as f64 / 720.0;
            let minutes = (30.0 * 2f64.powf(-1.5 * progress) * (0.5 * draws.gauss()).exp()).round().max(1.0);
            let ended = Timestamp::from_unix_seconds(end - ((30.0 - index as f64 / 24.0) * 86_400.0) as i64 - 7_200).iso();
            Run { cells: ["mechanical", "measured", "-", "-", "-"].map(str::to_owned), tier: "medium".into(), minutes: Some(minutes), eta_min: Some(30.0), first: true, ended, ..Run::default() }
        })
        .collect();
    let now = Timestamp::from_unix_seconds(end);
    let pairs = walk_forward_eta(&knobs, &runs, &since_start("7d", now), &now.iso());
    let share = |hits: usize| hits as f64 / pairs.len() as f64;
    let raw = share(pairs.iter().filter(|(run, eta)| run.minutes.unwrap() <= *eta).count());
    let calibrated = share(
        pairs
            .iter()
            .filter(|(run, eta)| {
                let ended = Timestamp::parse_loose(&run.ended).unwrap();
                let scale = calibrate_eta_pred(&knobs, &runs, &since_start("7d", ended), &run.ended).into_iter().find(|row| row.cell == "all").and_then(|row| row.value).map(super::forward::Figure::real);
                run.minutes.unwrap() <= scaled(*eta, scale, true)
            })
            .count(),
    );
    assert!(!pairs.is_empty() && (calibrated - 0.75).abs() < (raw - 0.75).abs(), "raw {raw} calibrated {calibrated} over {}", pairs.len());
}
