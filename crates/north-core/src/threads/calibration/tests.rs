use std::collections::BTreeMap;

use rusqlite::Connection;

use super::boxes::{BoxRun, box_chains, box_duration};
use super::forward::{Figure, calibrate_box, calibrate_eta_pred, walk_forward_eta};
use crate::usage::decide::world::Rate;
use super::recommend::{asked_of, recommend, recommend_eta, recommend_usd};
use super::verbs::{eta_line, since_start};
use super::{Knobs, Run, median, quantile, scaled, tier_key, tier_rank};
use crate::foundation::clock::Timestamp;
use crate::threads::runs::brief::Fields;

fn run(tier: &str, ok: bool, cost: Option<f64>) -> Run {
    Run { cells: ["research", "measured", "-", "-", "-"].map(str::to_owned), tier: tier.to_owned(), ok, cost_usd: cost, api_usd: cost, outcome: if ok { "done" } else { "not done" }.to_owned(), ..Run::default() }
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
    let mut runs = many(15, "claude:haiku", 15, 1.0);
    runs.extend(many(15, "codex:sol-medium", 15, 0.5));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], true);
    assert_eq!((tier.as_deref(), rule.as_str()), (Some("codex:sol-medium"), "$ per success"));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], false);
    assert_eq!((tier.as_deref(), rule.as_str()), (Some("claude:haiku"), "rank"));
}

#[test]
fn recommend_falls_back_to_rank_and_says_which_tiers_have_thin_dollars() {
    let mut runs = many(15, "claude:haiku", 15, 1.0);
    runs.extend((0..15).map(|_| run("codex:sol-medium", true, None)));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], true);
    assert_eq!((tier.as_deref(), rule.as_str()), (Some("claude:haiku"), "rank (uncalibrated: thin $ codex:sol-medium n=0 < 15)"));
}

#[test]
fn recommend_keeps_the_incumbent_unless_a_dearer_challenger_beats_it_by_the_margin() {
    let mut runs = many(30, "claude:haiku", 21, 1.0);
    runs.extend(many(30, "claude:opus-medium", 27, 1.0));
    let (cell, _, tier, _) = pick(&runs, "Category: research", &[], true);
    assert_eq!(tier.as_deref(), Some("claude:opus-medium"));
    let (_, _, held, _) = pick(&runs, "Category: research", &[(&cell, "haiku")], true);
    assert_eq!(held.as_deref(), Some("claude:haiku"));
    let mut sure = many(30, "claude:opus-medium", 30, 1.0);
    sure.extend(many(30, "claude:haiku", 21, 1.0));
    let (_, _, moved, _) = pick(&sure, "Category: research", &[(&cell, "claude:haiku")], true);
    assert_eq!(moved.as_deref(), Some("claude:opus-medium"));
}

#[test]
fn recommend_with_no_passing_tier_takes_the_lowest_measured_cost_or_the_workers_skill_default() {
    with_no_passing_tier_the_pick_is_the_lowest_measured_cost_among_sampled_tiers_for_any_run_mix();
    let mut runs = many(15, "claude:haiku", 3, 1.0);
    runs.extend(many(15, "codex:sol-medium", 6, 1.0));
    let (_, _, tier, rule) = pick(&runs, "Category: research", &[], true);
    assert_eq!(tier.as_deref(), Some("codex:sol-medium"));
    assert!(rule.starts_with("$ per expected success (no tier passes"));
    let unpriced: Vec<Run> = (0..5).map(|_| run("claude:haiku", false, None)).collect();
    let (_, _, tier, rule) = pick(&unpriced, "Category: research", &[], true);
    assert_eq!(tier.as_deref(), Some("claude:opus-medium"));
    assert!(rule.starts_with("workers-skill default for research"));
}

#[test]
fn a_knob_override_moves_the_pick_without_touching_the_registry() {
    let runs = many(3, "claude:haiku", 3, 1.0);
    let held = BTreeMap::new();
    let strict = recommend(&Knobs::registry(), &runs, &asked("Category: research"), &held, true);
    let relaxed = recommend(&Knobs { min_runs: 3, usd_min: 3, ..Knobs::registry() }, &runs, &asked("Category: research"), &held, true);
    assert_eq!((strict.tier.as_deref(), relaxed.tier.as_deref()), (Some("claude:opus-medium"), Some("claude:haiku")));
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
fn the_eta_line_gives_the_active_minute_spread_with_wall_p75_and_median_wait_and_falls_back_to_wall_alone() {
    let mut runs = timed(&[100.0, 200.0, 300.0, 400.0, 500.0]);
    for (run, (active, wait)) in runs.iter_mut().zip([(10.0, 90.0), (15.0, 185.0), (20.0, 280.0), (25.0, 375.0), (60.0, 440.0)]) {
        run.active_min = Some(active);
        run.wait_min = Some(wait);
    }
    let refs: Vec<&Run> = runs.iter().collect();
    let knobs = Knobs::registry();
    let eta = recommend_eta(&knobs, &refs, "Category: mechanical", "medium");
    assert_eq!(eta_line(&eta, eta.wall_p75.unwrap(), "claude:opus-medium", "uncalibrated"), "eta active 20m (p25 15\u{2013}p90 60, n=5) wall p75 400m wait median 280m (cell Category: mechanical, tier claude:opus-medium, uncalibrated, wall n=5)");
    let few: Vec<&Run> = runs.iter().take(2).collect();
    let thin = recommend_eta(&Knobs { eta_min_samples: 3, ..Knobs::registry() }, &few, "Category: mechanical", "medium");
    assert_eq!(eta_line(&thin, 200.0, "claude:opus-medium", "uncalibrated"), "eta wall 200m (p75 of n=2, cell Category: mechanical, tier claude:opus-medium, uncalibrated; too few runs with active minutes)");
}

#[test]
fn eta_prediction_backs_off_a_facet_until_enough_samples_and_ignores_other_tiers() {
    eta_p75_takes_the_third_of_three_durations_and_backs_off_the_two_run_scope_cell();
    let mut runs = timed(&[10.0, 20.0, 30.0, 99.0]);
    runs[3].tier = "claude:haiku".into();
    runs[0].cells[1] = "measured".into();
    let refs: Vec<&Run> = runs.iter().collect();
    let knobs = Knobs::registry();
    let wall = |cell: &str, tier: &str| {
        let eta = recommend_eta(&knobs, &refs, cell, tier);
        (eta.wall_p75, eta.wall_n, eta.cell)
    };
    assert_eq!(wall("Category: mechanical, Spec: measured", "claude:opus-medium"), (Some(30.0), 3, "Category: mechanical".to_owned()));
    assert_eq!(wall("Category: mechanical", "haiku"), (Some(99.0), 1, "Category: mechanical".to_owned()));
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
    let wider = box_chains(&Knobs { box_chain_gap_min: 180.0, ..knobs }, &rows);
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
    let args = crate::threads::verb::Args::parse(&[], &["--since"], &[]).ok().unwrap();
    let fields = Fields::read("Category: research, Spec: measured");
    let text = match super::verbs::recommend_with(&args, &context, &fields, &configuration) {
        Ok(text) => text,
        Err(_) => panic!("recommend refused"),
    };
    assert_eq!(
        text,
        "tier claude:haiku  cell [Category: research, Spec: measured]  n 5\n\
         success threshold 80%  rule rank (one tier passes)\n\
         success claude:haiku 100% shrunk (n=5, 5 ok, toward the category's pooled 100% with a prior of 15 runs)\n\
         \x20 claude:haiku       100% of 5  shrunk 100%  $ n/a (priced n=0)\n\
         eta wall 40m (p75 of n=5, cell Category: research, Spec: measured, tier claude:haiku, uncalibrated: no eta_pred calibration; too few runs with active minutes)  $0.50 (median of n=5, uncalibrated: no usd calibration)\n"
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
        let eta = recommend_eta(&knobs, &refs, cell, "medium");
        assert_eq!((eta.wall_p75, eta.wall_n, eta.cell.as_str()), (Some(20.0), 3, "Category: research, Spec: measured"), "{cell}");
        assert_eq!((eta.active, eta.wait_median), (None, None));
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
        let expected = found.stats.iter().filter(|(_, stat)| stat.n >= knobs.min_runs && stat.usd.is_some()).min_by(|(x, a), (y, b)| a.usd.partial_cmp(&b.usd).unwrap().then(tier_rank(x).cmp(&tier_rank(y)))).map(|(tier, _)| tier.clone()).or_else(|| Some("claude:opus-medium".to_owned()));
        assert_eq!(found.tier, expected, "case {case}");
        if let Some(tier) = found.tier.as_ref().filter(|_| found.rule.starts_with("$ per expected success")) {
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

fn tooling(count: usize, tier: &str, wins: usize, cost: f64) -> Vec<Run> {
    many(count, tier, wins, cost).into_iter().map(|run| Run { cells: ["tooling", "exact", "-", "-", "-"].map(str::to_owned), ..run }).collect()
}

#[test]
fn an_escalation_only_tier_is_never_a_first_assignment_however_cheap_or_sure() {
    for escalation in ["claude:opus-high", "claude:opus-xhigh"] {
        let mut runs = tooling(30, escalation, 30, 0.01);
        runs.extend(tooling(20, "claude:sonnet-high", 12, 1.0));
        runs.extend(tooling(20, "claude:opus-medium", 13, 2.0));
        let (_, _, tier, rule) = pick(&runs, "Category: tooling, Spec: exact", &[], true);
        assert_eq!(tier.as_deref(), Some("claude:sonnet-high"), "{escalation}: {rule}");
        let (_, _, held, _) = pick(&runs, "Category: tooling, Spec: exact", &[("Category: tooling, Spec: exact", escalation)], true);
        assert_eq!(held.as_deref(), Some("claude:sonnet-high"), "{escalation} held");
        let alone = tooling(30, escalation, 30, 0.01);
        let (_, _, tier, rule) = pick(&alone, "Category: tooling, Spec: exact", &[], true);
        assert_eq!((tier.as_deref(), rule.as_str()), (Some("claude:sonnet-high"), "workers-skill default for tooling (no tier passes; no first-assignment tier has 15 priced runs)"));
    }
}

#[test]
fn a_tier_under_the_minimum_runs_cannot_win_on_cost_and_success_is_shrunk_toward_the_pooled_rate() {
    let mut runs = tooling(14, "claude:haiku", 9, 0.01);
    runs.extend(tooling(32, "claude:sonnet-high", 19, 1.0));
    runs.extend(tooling(20, "claude:opus-medium", 13, 2.0));
    let found = recommend(&Knobs::registry(), &runs, &asked("Category: tooling, Spec: exact"), &BTreeMap::new(), true);
    assert_eq!(found.tier.as_deref(), Some("claude:sonnet-high"), "{}", found.rule);
    assert_eq!(found.rule, "$ per expected success (no tier passes; cheapest first-assignment tier with 15 priced runs)");
    let stat = |tier: &str| found.stats.iter().find(|(key, _)| key == tier).unwrap().1.clone();
    assert_eq!(stat("claude:haiku").usd, None);
    assert!((found.pooled - 41.0 / 66.0).abs() < 1e-12);
    let sonnet = stat("claude:sonnet-high");
    assert!((sonnet.shrunk - (19.0 + 15.0 * found.pooled) / 47.0).abs() < 1e-12);
    assert!((sonnet.usd.unwrap() - 1.0 / sonnet.shrunk).abs() < 1e-12);
    let mut grown = runs.clone();
    grown.extend(tooling(1, "claude:haiku", 1, 0.01));
    let found = recommend(&Knobs::registry(), &grown, &asked("Category: tooling, Spec: exact"), &BTreeMap::new(), true);
    assert_eq!(found.tier.as_deref(), Some("claude:haiku"), "{}", found.rule);
}

fn chain_ledger(rows: &str) -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../runs/fixtures/ledger.sql")).unwrap();
    db.execute_batch(&format!("INSERT INTO runs(agent, item, tier, category, started, ended, minutes, outcome, landed, expects_landing, role, brief_text) VALUES {rows};")).unwrap();
    db
}

#[test]
fn a_done_run_followed_by_another_run_on_the_item_within_a_day_is_not_kept_and_each_run_ranks_in_its_chain() {
    let db = chain_ledger(
        "('a1','north#1','sonnet','feature','2026-10-01T00:00:00Z','2026-10-01T01:00:00Z',60,'done',1,1,'worker',NULL),\
         ('a2','north#1','sonnet','feature','2026-10-01T23:30:00Z','2026-10-02T00:30:00Z',60,'done',1,1,'worker',NULL),\
         ('a3','north#1','sonnet','feature','2026-10-04T00:00:00Z','2026-10-04T01:00:00Z',60,'done',1,1,'worker',NULL),\
         ('b1','north#2','sonnet','feature','2026-10-01T00:00:00Z','2026-10-01T01:00:00Z',60,'done',1,1,'worker',NULL),\
         ('c1','north#3','sonnet','feature','2026-10-01T00:00:00Z','2026-10-01T01:00:00Z',60,'done',1,1,'worker',NULL),\
         ('c2','north#3','sonnet','feature','2026-10-02T01:00:01Z','2026-10-02T02:00:00Z',60,'done',1,1,'worker',NULL)",
    );
    let found = super::read::attempts(&db).unwrap();
    let ranks: Vec<(&str, usize)> = found.numbers.iter().map(|(agent, rank)| (agent.as_str(), *rank)).collect();
    assert_eq!(ranks, [("a1", 1), ("a2", 2), ("a3", 1), ("b1", 1), ("c1", 1), ("c2", 1)]);
    let followed = super::read::escalated(&db).unwrap();
    assert_eq!(followed.iter().map(String::as_str).collect::<Vec<_>>(), ["a1"]);
    let raws = super::read::raw_runs(&db, "1 = 1", &[]).unwrap();
    let kept = |agent: &str| raws.iter().find(|raw| raw.agent == agent).unwrap().succeeded(followed.contains(agent));
    assert_eq!((kept("a1"), kept("a2"), kept("a3"), kept("b1")), (false, true, true, true));
}

#[test]
fn reviewer_and_zero_minute_judge_runs_leave_the_worker_cells_in_every_reader() {
    let db = chain_ledger(
        "('w1','north#1','sonnet','feature','2026-10-01T00:00:00Z','2026-10-01T01:00:00Z',60,'done',1,1,'worker','Category: feature'),\
         ('r1','north#1','sonnet','unknown','2026-10-01T02:00:00Z','2026-10-01T02:10:00Z',10,'done',NULL,NULL,'worker','Item: north#1 Category: review'),\
         ('j1','north#1','haiku','unknown','2026-10-01T03:00:00Z','2026-10-01T03:00:20Z',0,'done',NULL,NULL,'worker',NULL),\
         ('m1','north#4','haiku','mechanical','2026-10-01T04:00:00Z','2026-10-01T04:00:20Z',0,'done',NULL,NULL,'worker',NULL),\
         ('u1','north#5','haiku','unknown','2026-10-01T05:00:00Z','2026-10-01T05:08:00Z',8,'done',NULL,NULL,'worker',NULL)",
    );
    let workers: Vec<String> = super::read::raw_runs(&db, "1 = 1", &[]).unwrap().into_iter().map(|raw| raw.agent).collect();
    assert_eq!(workers, ["w1", "m1", "u1"]);
    let roles: Vec<String> = db.prepare(&format!("SELECT {} FROM runs ORDER BY agent", crate::platform::run_roles::ROLE_SQL)).unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(roles, ["judge", "worker", "reviewer", "worker", "worker"]);
    let boxes: Vec<String> = super::read::box_rows(&db).unwrap().into_iter().map(|row| row.agent).collect();
    assert_eq!(boxes, ["w1", "m1", "u1"]);
    assert_eq!(super::read::attempts(&db).unwrap().numbers.get("w1"), Some(&1));
}

fn git(checkout: &std::path::Path, at: i64, args: &[&str]) -> String {
    let date = format!("@{at} +0000");
    let output = std::process::Command::new("git").arg("-C").arg(checkout).args(args).env("GIT_AUTHOR_DATE", &date).env("GIT_COMMITTER_DATE", &date).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn land(checkout: &std::path::Path, at: i64, files: &[&str], message: &str) -> String {
    for file in files {
        let path = checkout.join(file);
        let before = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(&path, format!("{before}{at}\n")).unwrap();
    }
    git(checkout, at, &["add", "-A"]);
    git(checkout, at, &["commit", "-q", "-m", message]);
    git(checkout, at, &["rev-parse", "HEAD"])
}

#[test]
fn rework_marks_reverted_fixed_forward_and_culprit_landings_within_48_hours_and_the_summary_counts_them() {
    use super::rework::{Cause, Rework, Sources};
    let directory = tempfile::tempdir().unwrap();
    let checkout = directory.path().join("code/proj/main");
    std::fs::create_dir_all(&checkout).unwrap();
    let hour = 3600;
    let t0 = Timestamp::parse("2026-10-20T00:00:00Z").unwrap().unix_seconds();
    git(&checkout, t0, &["init", "-q", "-b", "main"]);
    land(&checkout, t0, &["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs"], "Start");
    let reverted = land(&checkout, t0 + hour, &["a.rs"], "Add one");
    let fixed = land(&checkout, t0 + 2 * hour, &["b.rs", "c.rs"], "Add two");
    let culprit = land(&checkout, t0 + 3 * hour, &["d.rs"], "Add three");
    let late = land(&checkout, t0 + 4 * hour, &["e.rs"], "Add four (proj#9)");
    let followed = land(&checkout, t0 + 5 * hour, &["f.rs"], "Add five (proj#5)");
    let clean = land(&checkout, t0 + 6 * hour, &["c.rs"], "Add six");
    land(&checkout, t0 + 7 * hour, &["a.rs"], &format!("Revert \"Add one\"\n\nThis reverts commit {reverted}."));
    land(&checkout, t0 + 8 * hour, &["b.rs"], "Fix two's edge case");
    land(&checkout, t0 + 9 * hour, &["f.rs"], "Finish five\n\nFollows: proj#5");
    land(&checkout, t0 + 80 * hour, &["e.rs"], "Rework four, fixes proj#9");
    git(&checkout, t0, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let culprits = directory.path().join("red-main");
    std::fs::create_dir_all(&culprits).unwrap();
    std::fs::write(culprits.join(format!("tompassarelli_proj-12-{culprit}")), "").unwrap();
    let rows: Vec<String> = [("rw1", 1, &reverted), ("rw2", 2, &fixed), ("rw3", 3, &culprit), ("rw4", 9, &late), ("rw5", 5, &followed), ("rw6", 6, &clean)]
        .iter()
        .map(|(agent, number, sha)| format!("('{agent}','proj#{number}','high','bug-known-cause','2026-10-19T23:00:00Z','2026-10-20T10:00:00Z',60,30,'done',1,1,'worker','{sha}','exact','module','rust','local-test')"))
        .collect();
    let extra = format!("INSERT INTO runs(agent,item,tier,category,started,ended,minutes,eta_min,outcome,landed,expects_landing,role,landed_sha,spec,scope,surface,verify) VALUES {};", rows.join(","));
    let database = fixture_ledger(directory.path(), &extra);
    let sources = Sources { code_root: Some(directory.path().join("code")), culprits: Some(culprits) };
    let rework = Rework::read(&Connection::open(&database).unwrap(), &sources).unwrap();
    let causes: Vec<(&str, Option<Cause>)> = rework.judged.iter().map(|(agent, judged)| (agent.as_str(), judged.cause)).collect();
    assert_eq!(causes, [("rw1", Some(Cause::Reverted)), ("rw2", Some(Cause::FixedForward)), ("rw3", Some(Cause::Culprit)), ("rw4", None), ("rw5", Some(Cause::FixedForward)), ("rw6", None)]);
    assert_eq!(rework.rates(t0), BTreeMap::from([("proj".to_owned(), (4, 6))]));
    let outside = crate::threads::fake::Recorded::default();
    let context = crate::threads::verb::Context { database: &database, actor: "tester".into(), no_native: None, outside: &outside, now: Timestamp::parse("2026-10-25T00:00:00Z").unwrap(), replica: false };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = super::summary::run_with(&["--since".into(), "2026-10-19".into()], &context, &sources, &mut out, &mut err);
    assert_eq!((code, String::from_utf8(err).unwrap()), (0, String::new()));
    let text = String::from_utf8(out).unwrap();
    let header = text.lines().next().unwrap();
    let row = text.lines().find(|line| line.starts_with("bug-known-cause")).unwrap();
    let column = |name: &str| row[header.find(name).unwrap()..].split("  ").next().unwrap().trim().to_owned();
    assert_eq!((column("n "), column("ok"), column("rework "), column("revert/fix/culprit")), ("6".to_owned(), "33%".to_owned(), "4 66%".to_owned(), "1/2/1 of 6".to_owned()));
}

fn eta_sample(category: &str, tier: &str, predicted: f64, actual: f64) -> super::eta_calibration::Sample {
    super::eta_calibration::Sample { category: category.to_owned(), tier: tier_key(tier), predicted, actual }
}

#[test]
fn the_eta_table_gives_each_category_and_tier_its_median_log_error_and_its_share_within_half_to_double() {
    let samples = [
        eta_sample("tooling", "sonnet", 30.0, 30.0),
        eta_sample("tooling", "sonnet", 30.0, 60.0),
        eta_sample("tooling", "sonnet", 30.0, 120.0),
        eta_sample("feature", "medium", 40.0, 10.0),
    ];
    let table = super::eta_calibration::render(&samples);
    let rows: Vec<Vec<&str>> = table.lines().map(|line| line.split_whitespace().collect()).collect();
    assert_eq!(rows[0], ["category", "tier", "n", "median_abs_ln_error", "within_0.5_to_2x"]);
    assert_eq!(rows[1], ["feature", "claude:opus-medium", "1", "1.39", "0%"]);
    assert_eq!(rows[2], ["tooling", "claude:sonnet-high", "3", "0.69", "66%"]);
}

fn rate_db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT;
         CREATE TABLE run_costs(account TEXT NOT NULL, started TEXT NOT NULL, ended TEXT NOT NULL, agent TEXT NOT NULL, cost_usd REAL NOT NULL, PRIMARY KEY(account, started, ended, agent)) STRICT;
         CREATE TABLE calibration(at TEXT NOT NULL, kind TEXT NOT NULL, cell TEXT NOT NULL, n INTEGER NOT NULL, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT NOT NULL) STRICT;",
    )
    .unwrap();
    db
}

fn hours_of_use(db: &Connection, account: &str, hours: usize, five_hour_rise: f64, week_rise: f64, usd: f64) {
    for hour in 0..hours {
        let day = 1 + hour / 24;
        let at = |minute: u32| format!("2026-10-{day:02}T{:02}:{minute:02}:00Z", hour % 24);
        for (name, rise) in [("session", five_hour_rise), ("weekly_all", week_rise)] {
            for (minute, used) in [(0, 10.0), (30, 10.0 + rise)] {
                db.execute("INSERT INTO usage VALUES(?1, 'claude', ?2, 'window', ?3, ?4, 0, ?5, NULL, 'header')", rusqlite::params![at(minute), account, name, used, format!("2026-11-0{}T00:00:00Z", if name == "session" { 1 } else { 2 })]).unwrap();
            }
        }
        db.execute("INSERT INTO run_costs VALUES(?1, ?2, ?3, ?4, ?5)", rusqlite::params![format!("claude-{account}"), at(5), at(25), format!("w{hour}"), usd]).unwrap();
    }
}

#[test]
fn rates_land_per_account_and_window_with_n_and_the_pooled_fallback() {
    let db = rate_db();
    hours_of_use(&db, "a", 24, 4.0, 1.0, 2.0);
    hours_of_use(&db, "b", 5, 3.0, 0.5, 1.0);
    let rows = super::rates::calibrate_rates(&db, "2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z").unwrap();
    let find = |cell: &str| rows.iter().find(|row| row.cell == cell).unwrap_or_else(|| panic!("no {cell}: {rows:?}"));
    assert_eq!((find("a five_hour").n, find("a five_hour").value), (24, Some(Figure::Real(2.0))));
    assert_eq!((find("a week").n, find("a week").value), (24, Some(Figure::Real(0.5))));
    assert_eq!((find("b five_hour").n, find("b five_hour").value), (5, Some(Figure::Real(3.0))));
    assert_eq!((find("pooled five_hour").n, find("pooled five_hour").value), (29, Some(Figure::Real((24.0 * 4.0 + 5.0 * 3.0) / (24.0 * 2.0 + 5.0)))));
    assert_eq!(find("pooled week").n, 29);
    for row in &rows {
        db.execute("INSERT INTO calibration VALUES('2026-11-01T00:00:00Z', 'rate', ?1, ?2, NULL, NULL, NULL, ?3, 'measured')", rusqlite::params![row.cell, row.n as i64, row.value.unwrap().real()]).unwrap();
    }
    let own = super::rates_of(&db, "a").unwrap().unwrap();
    assert_eq!((own.five_hour, own.week), (Rate::Account { points_per_usd: 2.0, n: 24 }, Rate::Account { points_per_usd: 0.5, n: 24 }));
    let thin = super::rates_of(&db, "b").unwrap().unwrap();
    assert!(matches!(thin.five_hour, Rate::Pooled { n: 29, .. }) && matches!(thin.week, Rate::Pooled { n: 29, .. }), "{thin:?}");
}

#[test]
fn a_rise_across_a_window_reset_is_not_counted() {
    let samples = [("2026-10-01T00:10:00Z", 90.0, "r1"), ("2026-10-01T00:40:00Z", 3.0, "r2")]
        .map(|(at, used_pct, resets_at)| super::rates::Sample { account: "a".into(), window: "week", at: at.into(), used_pct, resets_at: resets_at.into() });
    let costs = [super::rates::Cost { account: "a".into(), at: "2026-10-01T00:20:00Z".into(), usd: 1.0 }];
    assert!(super::rates::measure(&samples, &costs).is_empty());
}
