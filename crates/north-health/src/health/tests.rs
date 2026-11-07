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

fn runs(north: &str) -> CiInput {
    let makgora = r#"[{"status":"completed","conclusion":"success","workflowName":"Cargo"}]"#;
    CiInput { runs: [("north".to_owned(), Ok(north.to_owned())), ("makgora".to_owned(), Ok(makgora.to_owned()))].into() }
}

#[test]
fn ci_reads_a_failure_conclusion_as_red_and_a_304_reply_a_skipped_run_and_a_cancelled_run_as_not_red() {
    let run = |name: &str, conclusion: &str| format!(r#"{{"status":"completed","conclusion":"{conclusion}","workflowName":"{name}"}}"#);
    assert_eq!(ci(runs(&format!("[{}]", run("Main", "failure")))), Err("north/Main: failure".into()));
    assert_eq!(ci(runs(&format!("[{}]", run("Main", "skipped")))), Ok(()), "a skipped run is not red");
    assert_eq!(ci(runs(&format!("[{}]", run("Main", "cancelled")))), Ok(()), "a cancelled run is not red");
    assert_eq!(ci(runs(&format!("[{},{}]", run("Cargo", "cancelled"), run("Cargo", "failure")))), Err("north/Cargo: failure".into()), "a cancelled run does not hide the failure under it");
    assert_eq!(ci(runs(&format!("[{},{}]", run("Cargo", "cancelled"), run("Cargo", "success")))), Ok(()));
    let mut not_modified = runs("[]");
    not_modified.runs.insert("north".into(), Err("gh exited 1: HTTP 304: Not Modified".into()));
    assert_eq!(ci(not_modified), Ok(()), "a 304 reply is no reading, not a red");
    let mut broken = runs("[]");
    broken.runs.insert("north".into(), Err("gh exited 1: HTTP 502".into()));
    assert!(ci(broken).is_err(), "another gh error still alarms");
}
