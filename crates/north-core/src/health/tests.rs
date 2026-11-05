use super::*;

#[test]
fn health_alerts_once_per_episode_after_two_consecutive_failing_ticks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("health.json");
    let mut state = State::default();
    let fail: Verdict = Err("stale".into());
    let tick = |state: &mut State, result: &Verdict, now| {
        let breaching = if result.is_err() { ["pace".to_owned()].into() } else { Default::default() };
        let confirmed = state.breaches.tick(&breaching).contains("pace");
        transition(state, "pace", result, confirmed, now)
    };
    assert_eq!(tick(&mut state, &fail, 99), None, "a single failing tick alerts no one");
    assert_eq!(tick(&mut state, &Ok(()), 100), None, "a lone breach that clears says nothing");
    assert_eq!(tick(&mut state, &fail, 100), None);
    save(&path, &state).unwrap();
    let mut state: State = serde_json::from_str(&read(&path).unwrap()).unwrap();
    assert_eq!(tick(&mut state, &fail, 101), Some("health: pace: stale".into()), "the second consecutive failing tick alerts once");
    assert_eq!(tick(&mut state, &fail, 102), None, "an unchanged failing tick posts nothing");
    assert_eq!(state.checks["pace"].since, Some(101));
    assert_eq!(tick(&mut state, &Ok(()), 103), Some("health: pace: recovered".into()));
    assert_eq!(tick(&mut state, &Ok(()), 104), None);
    assert_eq!(tick(&mut state, &fail, 105), None);
    assert_eq!(tick(&mut state, &fail, 106), Some("health: pace: stale".into()));
}

