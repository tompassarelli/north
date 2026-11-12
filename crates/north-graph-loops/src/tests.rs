use north_graph::Graph;
use serde_json::json;

use crate::{Cells, corrections, read_from, slow_test, tell_to as tell, ticks_from};

#[test]
fn a_tick_that_moves_a_cell_leaves_dispatch_only_the_new_value_and_the_new_tick() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first: Cells = [("eta_minutes opus feature", json!(92.0)), ("budget_usd opus feature", json!(12.67)), ("eta_minutes sonnet feature", json!(55.0))].into_iter().map(|(cell, value)| (cell.to_owned(), value)).collect();
    let empty = read_from(&graph, "tune").unwrap();
    tell(&mut graph, "tune", &empty, &first, 1_000).unwrap();
    let told = read_from(&graph, "tune").unwrap();
    assert_eq!((told.cells.clone(), told.at), (first.clone(), Some(1_000)));
    let mut second = first;
    second.insert("eta_minutes opus feature".into(), json!(141.0));
    second.remove("budget_usd opus feature");
    tell(&mut graph, "tune", &told, &second, 2_000).unwrap();
    let told = read_from(&graph, "tune").unwrap();
    assert_eq!((told.cells.clone(), told.at), (second, Some(2_000)));
    let effort: Cells = [("effort feature".to_owned(), json!("high"))].into_iter().collect();
    assert!(tell(&mut graph, "tune", &told, &effort, 3_000).is_err(), "an effort cell is outside the graph's vocabulary");
}

#[test]
fn a_finished_lease_leaves_admission_its_signature_s_newest_estimate_and_the_capacity_tick() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first: crate::Estimate = [("runs", 4.0), ("cores", 6.5), ("memory-mb", 9100.0), ("wall-s", 310.0)].into_iter().map(|(measure, value)| (measure.to_owned(), value)).collect();
    crate::tell_estimate_to(&mut graph, "a1", &first, 1_000).unwrap();
    crate::tell_estimate_to(&mut graph, "b2", &first, 1_500).unwrap();
    let mut second = first.clone();
    second.insert("cores".into(), 7.0);
    second.remove("wall-s");
    crate::tell_estimate_to(&mut graph, "a1", &second, 2_000).unwrap();
    assert_eq!(crate::estimate_from(&graph, "a1").unwrap(), second);
    assert_eq!(crate::estimate_from(&graph, "b2").unwrap(), first);
    assert_eq!(crate::estimate_from(&graph, "c3").unwrap(), crate::Estimate::new());
    assert_eq!(ticks_from(&graph).unwrap().get("capacity"), Some(&2_000));
    assert!(crate::tell_estimate_to(&mut graph, "a1", &[("flavour".to_owned(), 1.0)].into(), 3_000).is_err());
}

#[test]
fn a_reaper_tick_leaves_the_fleet_only_the_sessions_it_still_holds_and_the_new_dead_letter_count() {
    let mut graph = Graph::open_in_memory().unwrap();
    let verdicts = |held: &[(&str, &str)]| held.iter().map(|(name, verdict)| ((*name).to_owned(), (*verdict).to_owned())).collect();
    let first = crate::Retirement { verdicts: verdicts(&[("n1", "idle"), ("n2", "parked")]), dead_letters_24h: Some(3) };
    crate::tell_retirement_to(&mut graph, &first, 1_000).unwrap();
    assert_eq!(crate::retirement_from(&graph).unwrap(), first);
    let second = crate::Retirement { verdicts: verdicts(&[("n2", "stale")]), dead_letters_24h: Some(0) };
    crate::tell_retirement_to(&mut graph, &second, 2_000).unwrap();
    assert_eq!(crate::retirement_from(&graph).unwrap(), second);
    assert_eq!(ticks_from(&graph).unwrap().get("fleet"), Some(&2_000));
}

#[test]
fn a_distill_tick_leaves_the_detector_each_correction_once_and_a_flow_tick_the_tests_still_over_their_line() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first = [("pace".to_owned(), 1_000), ("pace".to_owned(), 2_000), ("overbuilt".to_owned(), 3_000)];
    corrections::tell_to(&mut graph, &first, 4_000).unwrap();
    let mut told = corrections::seen_from(&graph).unwrap();
    told.sort();
    assert_eq!(told, vec![("overbuilt".to_owned(), 3_000), ("pace".to_owned(), 1_000), ("pace".to_owned(), 2_000)]);
    corrections::tell_to(&mut graph, &first[..1], 5_000).unwrap();
    assert_eq!(corrections::seen_from(&graph).unwrap(), vec![("pace".to_owned(), 1_000)]);
    assert_eq!(ticks_from(&graph).unwrap().get("distill"), Some(&5_000));

    let slow = |seconds| slow_test::Slow { binary: "north::it".into(), name: "slow::sleeps".into(), seconds };
    slow_test::tell_to(&mut graph, &[slow(16.2)], 6_000).unwrap();
    slow_test::tell_to(&mut graph, &[slow(17.5)], 7_000).unwrap();
    assert_eq!(slow_test::read_from(&graph).unwrap(), vec![slow(17.5)]);
    slow_test::tell_to(&mut graph, &[], 8_000).unwrap();
    assert_eq!((slow_test::read_from(&graph).unwrap(), ticks_from(&graph).unwrap().get("flow").copied()), (vec![], Some(8_000)));
}

#[test]
fn a_recording_leaves_the_scoreboard_only_the_readings_it_told_and_its_tick() {
    use crate::scoreboard::{Reading, Told, read_from, tell_to};
    let reading = |number: &str, week: &str, figure| Reading { number: number.to_owned(), week: week.to_owned(), figure, observations: 12, higher_is_better: false };
    let mut graph = Graph::open_in_memory().unwrap();
    let first = [reading("corrections", "2026-W44", 0.1), reading("corrections", "2026-W45", 0.2), reading("waits", "2026-W45", 3.5)];
    tell_to(&mut graph, &first, 1_000).unwrap();
    assert_eq!(read_from(&graph).unwrap(), Told { readings: first.to_vec(), at: Some(1_000) });
    let second = [reading("corrections", "2026-W45", 0.2), reading("corrections", "2026-W46", 0.3)];
    tell_to(&mut graph, &second, 2_000).unwrap();
    assert_eq!(read_from(&graph).unwrap(), Told { readings: second.to_vec(), at: Some(2_000) });
}

#[test]
fn a_recording_of_one_repository_leaves_the_others_reviews_and_replaces_its_own_reasons() {
    use crate::code_health::{Review, read_from, tell_to};
    let review = |median, share, reasons: &[&str]| Review { hand_files_median: Some(median), revert_share: Some(share), reasons: reasons.iter().map(|reason| (*reason).to_owned()).collect() };
    let mut graph = Graph::open_in_memory().unwrap();
    tell_to(&mut graph, "north", &review(3.0, 0.1, &["12 commits since review", "hotspot a.rs"]), 1_000).unwrap();
    tell_to(&mut graph, "wisp", &review(2.0, 0.0, &[]), 1_500).unwrap();
    tell_to(&mut graph, "north", &review(4.0, 0.1, &["hotspot a.rs"]), 2_000).unwrap();
    let told = read_from(&graph).unwrap();
    assert_eq!((told.reviews["north"].clone(), told.reviews["wisp"].clone(), told.at), (review(4.0, 0.1, &["hotspot a.rs"]), review(2.0, 0.0, &[]), Some(2_000)));
}

#[test]
fn a_learned_value_that_loses_to_its_constant_is_set_aside_and_one_that_wins_drives() {
    use crate::skill::{Decision, Skill, Verdict, tell_skills_to, verdict_from};
    let mut graph = Graph::open_in_memory().unwrap();
    assert_eq!(verdict_from(&graph, Decision::Eta).unwrap(), Verdict::Unjudged);
    let losing = Skill { decision: Decision::Eta, n: 1_400, learned: 31.0, constant: 27.0, value: Some(48.0) };
    let winning = Skill { decision: Decision::Budget, n: 1_400, learned: 1.2, constant: 1.9, value: Some(9.0) };
    let thin = Skill { decision: Decision::ArmShares, n: 12, learned: 9.0, constant: 4.0, value: None };
    tell_skills_to(&mut graph, &[losing.clone(), winning.clone(), thin], 1_000).unwrap();
    let eta = verdict_from(&graph, Decision::Eta).unwrap();
    assert_eq!((eta.drives(), eta.constant()), (false, Some(48.0)));
    assert!(eta.line().is_some_and(|line| line.contains("set aside")));
    assert_eq!(verdict_from(&graph, Decision::Budget).unwrap(), Verdict::Drives(winning));
    assert!(verdict_from(&graph, Decision::ArmShares).unwrap().drives(), "a reading under its minimum runs leaves the value driving");
    let recovered = Skill { learned: 25.0, ..losing };
    tell_skills_to(&mut graph, &[recovered], 2_000).unwrap();
    assert!(verdict_from(&graph, Decision::Eta).unwrap().drives(), "a value set aside drives again once it wins");
    assert_eq!(verdict_from(&graph, Decision::Budget).unwrap(), Verdict::Unjudged);
}
