use super::*;

const NOW: u64 = 1_800_000_000;

fn context(queued_for: u64) -> WatchContext {
    let created = NOW - queued_for;
    let (days, rest) = (created / 86_400, created % 86_400);
    let date = time::Date::from_julian_day(i32::try_from(days).unwrap() + 2_440_588).unwrap();
    let created_at = format!("{date}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60);
    let run = Run { name: "Cargo".into(), head_branch: "main".into(), event: "push".into(), status: "queued".into(), created_at, ..Run::default() };
    WatchContext { now: NOW, repos: vec![RepoFacts { name: "north".into(), runs: vec![run], ..RepoFacts::default() }], ..WatchContext::default() }
}

#[test]
fn a_run_queued_244_s_raises_no_alarm_and_one_past_the_queued_floor_does() {
    assert!(queued(&context(244), &context(244).repos[0]).is_none());
    let stuck = context(700);
    let alert = queued(&stuck, &stuck.repos[0]).expect("a run queued past the floor alarms");
    assert!(alert.summary.contains("queued over 600 s"), "{}", alert.summary);
}
