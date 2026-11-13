use super::*;

#[test]
fn a_burst_placed_from_a_slow_survey_spreads_over_the_build_hosts_and_keeps_none_on_the_control_host() {
    let mut recorded: Case = serde_json::from_str(include_str!("../../../crates/north-pacing/src/pacing/placement/fixtures/burst-2026-11-12.json")).unwrap();
    for machine in &mut recorded.machines {
        machine.has_cwd = None;
        machine.stale_age_s = Some(1.0);
        if machine.name == "gex45" && let Some(backlog) = machine.saturation.as_mut().and_then(|reading| reading.backlog.as_mut()) {
            backlog.load = 8.0;
            machine.hosted = 0;
        }
    }
    let launch = Launch { name: "t01-l21", account: "d", brief: None, cwd: Some(Path::new("/tmp/eval-b/worktrees/t01-l21")), machine: None, lead: false, role: Role::Worker, model: None, family: ModelFamily::Opus, operator: false, repository: "" };
    let mut placed: BTreeMap<String, i64> = BTreeMap::new();
    for _ in 0..21 {
        let mut case = recorded.clone();
        for machine in &mut case.machines {
            machine.placed_recently = placed.get(&machine.name).copied().unwrap_or(0);
        }
        let decision = decide(&case, &launch, &Overrides::default()).unwrap();
        *placed.entry(decision.machine.map_or_else(|| "here".to_owned(), |machine| machine.name)).or_insert(0) += 1;
    }
    let on = |host: &str| placed.get(host).copied().unwrap_or(0);
    assert_eq!(on("here"), 0, "the ladder of 2026-11-12 4:35 AM kept 18 of 21 here because a survey that never asked read the directory as missing: {placed:?}");
    assert!(["gex45", "cpx62", "cpx62b"].iter().all(|host| on(host) > 0), "a launch kept here is not counted on any host, so the next aims at the same one: {placed:?}");
}
