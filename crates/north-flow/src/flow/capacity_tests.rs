use super::*;

fn status(limit: f64, committed: f64, holding: &str) -> String {
    format!(r#"{{"aggregateCpuLimit":{limit},"committedBatchCpus":{committed},"queued":[],"holding":[{holding}]}}"#) + "\n---\n 300G\n"
}

#[test]
fn a_replay_of_the_7_40_am_host_facts_alarms_once_the_laptop_has_carried_fleet_builds_for_five_minutes_north_1036() {
    let build = r#"{"class":"heavy","owner":"gate:n1001","committedCpus":5.5}"#;
    let laptop = status(22.0, 22.0, &[build; 4].join(","));
    let readings = vec![
        parse("whiterabbit", &laptop).unwrap(),
        parse("gex45", &status(20.0, 6.3, "")).unwrap(),
        parse("cpx62", &status(16.0, 0.8, "")).unwrap(),
    ];
    let builders = vec!["gex45".to_owned(), "cpx62".to_owned()];
    let (state, early) = spill_plan(&readings, &builders, &State::default(), 0.0);
    assert!(early.is_empty(), "no alarm before the laptop has carried the load for five minutes");
    let (_, actions) = spill_plan(&readings, &builders, &state, 301.0);
    assert_eq!(actions.len(), 1, "on 2026-11-10 at 7:40 AM the laptop ran fleet builds at load 22 of 24 while gex45 sat at 6.3 of 20 and cpx62 at 0.8 of 16, and nothing told anyone");
    assert_eq!(actions[0].repo, "whiterabbit");
}
