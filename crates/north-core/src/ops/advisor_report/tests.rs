use super::stats::{PyRandom, Row, paired_diff};
use super::{Run, summarize, verdict};

fn runs(passes: &[(&str, &[Option<bool>])], usd: Option<f64>, codex: bool, advisor: bool) -> Vec<Run> {
    passes
        .iter()
        .flat_map(|(scenario, trials)| {
            trials.iter().enumerate().map(move |(trial, passed)| Run {
                scenario: (*scenario).to_owned(),
                passed: *passed,
                usd,
                wall_s: Some(10.0 + (trial + 1) as f64),
                codex,
                advisor_calls: f64::from(u8::from(advisor)),
                advisor_usd: if advisor { 0.25 } else { 0.0 },
            })
        })
        .collect()
}

fn all(passed: bool, count: usize) -> Vec<Option<bool>> {
    vec![Some(passed); count]
}

fn rows(runs: &[Run]) -> Vec<Row> {
    runs.iter().map(|run| Row { scenario: run.scenario.clone(), passed: run.passed }).collect()
}

fn incumbent_and_challenger() -> (Vec<Run>, Vec<Run>) {
    let (t, f) = (Some(true), Some(false));
    (
        runs(&[("a", &[t, f]), ("b", &[t, t]), ("c", &[f, None])], Some(2.0), false, false),
        runs(&[("a", &[t, t]), ("b", &[t, t]), ("c", &[f, t])], Some(1.0), false, true),
    )
}

fn verdict_of(incumbent: Vec<Run>, challenger: Vec<Run>) -> &'static str {
    let (i, c) = (summarize(&incumbent, None), summarize(&challenger, None));
    verdict(&c, &i, paired_diff(&rows(&challenger), &rows(&incumbent), 4000), 0.1)
}

#[test]
fn the_generator_repeats_the_sequence_python_gives_for_seed_zero() {
    let mut rng = PyRandom::seeded_zero();
    let five: Vec<usize> = (0..12).map(|_| rng.below(5)).collect();
    assert_eq!(five, [3, 3, 0, 2, 4, 3, 3, 2, 3, 2, 4, 1]);
    let mut rng = PyRandom::seeded_zero();
    assert_eq!(rng.below(1), 0);
    let mut rng = PyRandom::seeded_zero();
    let last = (0..700).map(|_| rng.next_u32()).last();
    assert_eq!(last, Some(3_579_999_110));
}

#[test]
fn the_paired_interval_is_the_mean_task_difference_and_repeats_under_seed_zero() {
    let (incumbent, challenger) = incumbent_and_challenger();
    let first = paired_diff(&rows(&challenger), &rows(&incumbent), 4000);
    assert_eq!(first.0, (0.5 + 0.0 + 0.5) / 3.0);
    assert_eq!(paired_diff(&[], &[], 4000), (0.0, -1.0, 1.0));
}

#[test]
fn a_run_is_costed_per_run_and_per_win_with_advisor_use() {
    let (incumbent, challenger) = incumbent_and_challenger();
    assert_eq!(summarize(&incumbent, None).wilson, super::wilson(0.6, 5));
    let s = summarize(&challenger, None);
    assert_eq!((s.n, s.list_run, s.list_success), (6, Some(1.0), Some(6.0 / 5.0)));
    assert!((s.plan_run.unwrap() - 0.052).abs() < 1e-12);
    assert_eq!((s.wall_median, s.advisor_mean, s.advisor_share_runs, s.advisor_usd_share), (Some(11.5), 1.0, 1.0, Some(0.25)));
}

#[test]
fn codex_runs_spread_the_plan_dollars_per_run_and_stay_unpriced_without_them() {
    let codex = runs(&[("a", &[Some(true), Some(true)]), ("b", &[Some(true), Some(false)])], None, true, false);
    let s = summarize(&codex, Some(6.0));
    assert_eq!((s.list_run, s.plan_run, s.plan_success), (None, Some(1.5), Some(2.0)));
    assert_eq!(summarize(&codex, None).plan_success, None);
}

#[test]
fn a_challenger_beats_matches_for_less_gains_nothing_or_stays_undecided_by_pass_rate_and_cost() {
    let four = ["a", "b", "c", "d"];
    let cases = |passed: bool, count: usize, usd: f64| {
        let trials = all(passed, count);
        let spec: Vec<(&str, &[Option<bool>])> = four.iter().map(|scenario| (*scenario, trials.as_slice())).collect();
        runs(&spec, Some(usd), false, false)
    };
    assert_eq!(verdict_of(cases(false, 5, 1.0), cases(true, 5, 1.0)), "BEATS");
    assert_eq!(verdict_of(cases(true, 2, 2.0), cases(true, 2, 1.0)), "MATCHES-FOR-LESS");
    assert_eq!(verdict_of(cases(true, 2, 1.0), cases(true, 2, 2.0)), "NO-GAIN");
    let (t, f) = (Some(true), Some(false));
    let low = runs(&[("a", &[t]), ("b", &[f])], Some(1.0), false, false);
    let high = runs(&[("a", &[f]), ("b", &[t])], Some(1.0), false, false);
    assert_eq!(verdict_of(low, high), "UNDECIDED");
}
