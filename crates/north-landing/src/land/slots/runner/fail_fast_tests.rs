use super::*;

fn row(started: u64, landing: &str, command: &str, outcome: Outcome, cpu: f64) -> Record {
    let exit = match outcome {
        Outcome::Passed => 0,
        Outcome::Failed => 1,
        Outcome::Cancelled => 143,
    };
    Record { started, landing: landing.into(), paths: "crates".into(), outcome, exit, wall: cpu / 4.0, cpu: Some(cpu), command: command.into() }
}

fn commands(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn an_unrecorded_or_cheap_check_always_finishes_and_expensive_ones_rank_by_fails_per_cpu_second() {
    let mut history = Vec::new();
    for landing in 0..6u64 {
        let name = landing.to_string();
        history.push(row(landing * 10, &name, "lint", Outcome::Passed, 5.0));
        history.push(row(landing * 10 + 1, &name, "build", if landing % 2 == 0 { Outcome::Failed } else { Outcome::Passed }, 600.0));
        history.push(row(landing * 10 + 2, &name, "suite", if landing == 0 { Outcome::Failed } else { Outcome::Passed }, 400.0));
    }
    let plan = Plan::of(&commands(&["suite", "build", "lint", "new"]), "crates", &history);
    assert_eq!(plan.cheap, commands(&["lint", "new"]));
    assert_eq!(plan.expensive, commands(&["build", "suite"]), "build fails 4 in 8 per 600 CPU-s, suite 2 in 8 per 400 CPU-s");
}

#[test]
fn cancelled_rows_count_toward_neither_fail_rate_nor_cost() {
    let mut history = vec![row(0, "a", "build", Outcome::Passed, 600.0), row(1, "b", "build", Outcome::Passed, 600.0)];
    history.extend((2..12).map(|at| row(at, &at.to_string(), "build", Outcome::Cancelled, 1.0)));
    let estimate = estimate(&history, "build", "crates").unwrap();
    assert!((estimate.passed_cpu - 600.0).abs() < f64::EPSILON);
    assert!((estimate.fail_rate - 0.25).abs() < f64::EPSILON);
}

#[test]
fn replay_puts_the_likely_failure_first_and_keeps_passing_wall() {
    let mut records = Vec::new();
    for landing in 0..10u64 {
        let name = landing.to_string();
        let start = landing * 100;
        records.push(row(start, &name, "lint", Outcome::Passed, 5.0));
        records.push(row(start + 1, &name, "suite", Outcome::Passed, 400.0));
        records.push(row(start + 2, &name, "build", if landing % 3 == 0 { Outcome::Passed } else { Outcome::Failed }, 200.0));
    }
    let replay = replay(&records);
    assert_eq!((replay.failing, replay.passing), (6, 4));
    assert!((replay.today_cpu - 605.0).abs() < f64::EPSILON, "{replay:?}");
    assert!((replay.ordered_cpu - 205.0).abs() < f64::EPSILON, "{replay:?}");
    assert!((replay.ordered_wall - replay.today_wall).abs() < f64::EPSILON);
}
