use super::*;
use serde_json::json;

fn sample(text: &str, age: u64) -> FreshnessInput {
    FreshnessInput {
        now: timestamp("2026-10-15T10:00:00Z").unwrap() as u64 + age,
        text: Ok(text.into()),
    }
}

#[test]
fn health_runtime_allows_promotion_gap_and_reports_stale_mismatch() {
    let sha = "a".repeat(40);
    let input = |now, matching, fetched| RuntimeInput {
        now,
        current: Ok(format!(
            "/runtime/{}",
            if matching {
                sha.clone()
            } else {
                "b".repeat(40)
            }
        )),
        expected: Ok(sha.clone()),
        fetched,
    };
    let mut state = State::default();
    assert!(runtime(input(100, true, true), &mut state).is_ok());
    assert!(runtime(input(100, false, true), &mut state).is_ok());
    let mut state: State = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert!(runtime(input(999, false, true), &mut state).is_ok());
    assert!(runtime(input(1000, false, true), &mut state).is_err());
    assert!(
        runtime(input(1000, false, false), &mut state)
            .unwrap_err()
            .contains("fetch failed")
    );
    assert!(runtime(input(1001, true, false), &mut state).is_ok());
    assert_eq!(state.runtime_mismatch_since, None);
    assert!(runtime(input(1002, false, true), &mut state).is_ok());
}

#[test]
fn health_pace_uses_newest_history_sample_and_rejects_stale_or_missing() {
    let text = json!({"history": [
        {"ts": "2026-10-15T10:00:00Z"},
        {"ts": "2026-10-15T09:00:00Z"}
    ]})
    .to_string();
    assert!(pace(sample(&text, 179)).is_ok());
    assert!(pace(sample(&text, 180)).is_err());
    assert!(pace(sample("{}", 0)).is_err());
    assert!(pace(sample("invalid", 0)).is_err());
}

#[test]
fn health_usage_rejects_stale_empty_or_failed_readout() {
    assert!(usage(sample("2026-10-15T10:00:00.123456+00:00", 0)).is_ok());
    assert!(usage(sample("2026-10-15T10:00:00.123456+00:00\n", 599)).is_ok());
    assert!(usage(sample("2026-10-15T10:00:00Z\n", 600)).is_err());
    assert!(usage(sample("2026-10-15T12:00:00+02:00", 599)).is_ok());
    assert!(usage(sample("", 0)).is_err());
    assert!(
        usage(FreshnessInput {
            now: 0,
            text: Err("sqlite3 failed".into())
        })
        .is_err()
    );
}

#[test]
fn health_timers_checks_only_declared_user_timers_and_rejects_inactive() {
    let config = "capacity-watchdog = { rest = { system = [\n\"system.timer\"\n];\nuser = [\n\"one.timer\"\n\"worker.service\"\n\"two.timer\"\n];\n}; };";
    let mut input = TimersInput {
        config: Ok(config.into()),
        active: BTreeMap::from([
            ("one.timer".into(), Ok("active\n".into())),
            ("two.timer".into(), Ok("active\n".into())),
        ]),
    };
    assert!(
        timers(TimersInput {
            config: input.config.clone(),
            active: input.active.clone()
        })
        .is_ok()
    );
    input
        .active
        .insert("two.timer".into(), Ok("inactive\n".into()));
    assert_eq!(timers(input).unwrap_err(), "inactive or missing: two.timer");
    assert!(
        timers(TimersInput {
            config: Ok("user = [];".into()),
            active: BTreeMap::new()
        })
        .is_err()
    );
}

#[test]
fn health_ci_ignores_running_and_older_runs_but_reports_newest_completed_failure() {
    let rows = json!([
        {"status":"in_progress", "workflowName":"build", "conclusion":null},
        {"status":"completed", "workflowName":"build", "conclusion":"success"},
        {"status":"completed", "workflowName":"build", "conclusion":"failure"},
        {"status":"completed", "workflowName":"harness", "conclusion":"success"}
    ]);
    let mut runs = BTreeMap::from([
        ("north".into(), Ok(rows.to_string())),
        ("smashcraft".into(), Ok(rows.to_string())),
    ]);
    assert!(ci(CiInput { runs: runs.clone() }).is_ok());
    let mut failing = rows.clone();
    failing[1]["conclusion"] = json!("failure");
    runs.insert("north".into(), Ok(failing.to_string()));
    assert!(ci(CiInput { runs }).unwrap_err().contains("north/build"));
    assert!(
        ci(CiInput {
            runs: BTreeMap::from([
                ("north".into(), Ok("[]".into())),
                ("smashcraft".into(), Ok(rows.to_string())),
            ])
        })
        .is_err()
    );
}

#[test]
fn health_land_caches_success_and_failure_for_thirty_minutes() {
    let mut state = State::default();
    assert!(
        land(
            LandInput {
                now: 100,
                output: Some(Ok(String::new()))
            },
            &mut state
        )
        .is_ok()
    );
    assert!(!land_due(1899, &state));
    assert!(
        land(
            LandInput {
                now: 1899,
                output: None
            },
            &mut state
        )
        .is_ok()
    );
    assert!(land_due(1900, &state));
    assert!(
        land(
            LandInput {
                now: 1900,
                output: Some(Err("safe-push failed".into()))
            },
            &mut state
        )
        .is_err()
    );
    assert!(!land_due(1901, &state));
    assert_eq!(
        land(
            LandInput {
                now: 1901,
                output: None
            },
            &mut state
        )
        .unwrap_err(),
        "safe-push failed"
    );
    assert!(
        land(
            LandInput {
                now: 3700,
                output: Some(Ok(String::new()))
            },
            &mut state
        )
        .is_ok()
    );
}

#[test]
fn health_alerts_once_per_episode() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("health.json");
    let mut state = State::default();
    let fail = Err("stale".into());
    assert_eq!(
        transition(&mut state, "pace", &fail, 100),
        Some("health: pace: stale".into())
    );
    save(&path, &state).unwrap();
    let mut state: State = serde_json::from_str(&read(&path).unwrap()).unwrap();
    assert_eq!(transition(&mut state, "pace", &fail, 101), None);
    assert_eq!(state.checks["pace"].since, Some(100));
    assert_eq!(
        transition(&mut state, "pace", &Ok(()), 102),
        Some("health: pace: recovered".into())
    );
    assert_eq!(transition(&mut state, "pace", &Ok(()), 103), None);
    assert_eq!(
        transition(&mut state, "pace", &fail, 104),
        Some("health: pace: stale".into())
    );
    assert_eq!(state.checks["pace"].since, Some(104));
}
