use super::*;
use crate::platform::rolling_issue;
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
        ("makgora".into(), Ok(rows.to_string())),
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
                ("makgora".into(), Ok(rows.to_string())),
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

fn shown(active: &str, result: &str, invocation: &str) -> Reading {
    Reading::parse(&format!("LoadState=loaded\nActiveState={active}\nResult={result}\nInvocationID={invocation}\n"))
}

struct Recorder {
    open: Option<rolling_issue::Issue>,
    legacy: Vec<(u64, String)>,
    created: Vec<(String, String)>,
    edited: Vec<(u64, String)>,
    closed: Vec<(u64, String)>,
}

impl rolling_issue::Tracker for Recorder {
    fn open(&self, _: &str) -> Result<Option<rolling_issue::Issue>, String> {
        Ok(self.open.as_ref().map(|issue| rolling_issue::Issue { number: issue.number, body: issue.body.clone() }))
    }

    fn body(&self, number: u64) -> Result<String, String> {
        Err(format!("issue {number} is not read here"))
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(self.legacy.clone())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        self.created.push((title.into(), body.into()));
        Ok(())
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        self.edited.push((number, body.into()));
        Ok(())
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        self.closed.push((number, comment.into()));
        Ok(())
    }
}

fn recorder(open: Option<rolling_issue::Issue>) -> Recorder {
    Recorder { open, legacy: Vec::new(), created: Vec::new(), edited: Vec::new(), closed: Vec::new() }
}

#[test]
fn health_unit_failing_twice_in_a_row_is_judged_failing_until_a_run_succeeds() {
    let unit = "agents-flow-stall.service";
    let mut runs = BTreeMap::new();
    assert!(units::judge(unit, Some(&shown("failed", "exit-code", "run-1")), &mut runs).is_ok());
    assert!(units::judge(unit, Some(&shown("failed", "exit-code", "run-1")), &mut runs).is_ok());
    let error = units::judge(unit, Some(&shown("failed", "exit-code", "run-2")), &mut runs).unwrap_err();
    assert_eq!(error, "agents-flow-stall.service failed 2 runs in a row");
    assert!(units::judge(unit, Some(&shown("inactive", "success", "run-3")), &mut runs).is_ok());
}

#[test]
fn health_failing_units_share_one_rolling_issue_per_host_that_it_edits_and_closes() {
    let two = [("a.service".to_owned(), "a.service failed 2 runs in a row".to_owned()), ("b.service".to_owned(), "b.service failed 3 runs in a row".to_owned())];
    let mut breaches = rolling_issue::Breaches::default();
    let mut tracker = recorder(None);
    units::reconcile(&mut tracker, &mut breaches, "whiterabbit", &two).unwrap();
    assert!(tracker.created.is_empty() && tracker.edited.is_empty(), "a single breaching tick posts nothing");
    units::reconcile(&mut tracker, &mut breaches, "whiterabbit", &two).unwrap();
    assert_eq!(tracker.created.len(), 1, "the second consecutive breach opens one issue for two units, not two");
    assert_eq!(tracker.created[0].0, "Harness services failing on whiterabbit");
    let body = tracker.created[0].1.clone();
    assert!(body.contains("`a.service`") && body.contains("`b.service`") && body.contains("\nNeeds: laptop\n"));

    let mut tracker = recorder(Some(rolling_issue::Issue { number: 7, body }));
    units::reconcile(&mut tracker, &mut breaches, "whiterabbit", &two).unwrap();
    assert!(tracker.created.is_empty() && tracker.edited.is_empty() && tracker.closed.is_empty(), "an unchanged set touches nothing");

    units::reconcile(&mut tracker, &mut breaches, "whiterabbit", &two[..1]).unwrap();
    assert_eq!(tracker.edited.len(), 1);
    assert_eq!(tracker.edited[0].0, 7);
    assert!(!tracker.edited[0].1.contains("b.service"));

    units::reconcile(&mut tracker, &mut breaches, "whiterabbit", &[]).unwrap();
    assert_eq!(tracker.closed.len(), 1);
    assert_eq!(tracker.closed[0].0, 7);

    let mut breaches = rolling_issue::Breaches::default();
    let mut none = recorder(None);
    units::reconcile(&mut none, &mut breaches, "whiterabbit", &[]).unwrap();
    assert!(none.created.is_empty() && none.closed.is_empty());
}

#[test]
fn health_code_health_review_unit_alerts_on_its_first_failure_and_a_success_between_failures_resets_the_count() {
    let mut runs = BTreeMap::new();
    assert!(units::judge("code-health-review.service", Some(&shown("failed", "exit-code", "run-1")), &mut runs).is_err());
    let other = "lane-gc.service";
    assert!(units::judge(other, Some(&shown("failed", "exit-code", "run-1")), &mut runs).is_ok());
    assert!(units::judge(other, Some(&shown("inactive", "success", "run-2")), &mut runs).is_ok());
    assert!(units::judge(other, Some(&shown("failed", "exit-code", "run-3")), &mut runs).is_ok());
}

#[test]
fn health_unit_without_a_loaded_service_is_not_judged() {
    let mut runs = BTreeMap::new();
    let missing = Reading::parse("LoadState=not-found\nActiveState=inactive\n");
    assert!(units::judge("x.service", Some(&missing), &mut runs).is_ok());
    assert!(runs.is_empty());
    assert!(units::judge("x.service", None, &mut runs).is_ok());
    assert!(runs.is_empty());
    assert_eq!(units::service_of("lane-gc.timer"), "lane-gc.service");
}

#[test]
fn health_closes_per_unit_issues_of_units_that_no_longer_fail_and_keeps_those_still_failing() {
    let failing = [("a.service".to_owned(), "a.service failed 2 runs in a row".to_owned())];
    let mut tracker = recorder(None);
    tracker.legacy = vec![
        (1, "Harness unit a.service keeps failing on whiterabbit".into()),
        (2, "Harness unit b.service keeps failing".into()),
        (3, "Harness unit c.service keeps failing on gex45".into()),
        (4, "Harness unit lane-gc.service ran slowly".into()),
    ];
    let mut breaches = rolling_issue::Breaches::default();
    units::reconcile(&mut tracker, &mut breaches, "whiterabbit", &failing).unwrap();
    assert_eq!(tracker.closed.iter().map(|(number, _)| *number).collect::<Vec<_>>(), [2]);
}

fn run_list(rows: &[(&str, &str, &str)]) -> String {
    let rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|(workflow, sha, conclusion)| serde_json::json!({"workflowName": workflow, "headSha": sha, "conclusion": conclusion, "status": "completed", "url": format!("https://runs/{workflow}")}))
        .collect();
    serde_json::to_string(&rows).unwrap()
}

#[test]
fn health_closes_the_main_red_issue_only_when_every_workflows_newest_run_is_green_beyond_the_culprit() {
    let body = "Intro\n\n## Done when\n- [ ] The newest Harness and Cargo runs on main are green on a commit\n\n<!-- main-red:1:1 -->\nCulprit range: tip, base\nFailing step: test\n";
    let issue = || Some(rolling_issue::Issue { number: 9, body: body.to_owned() });
    let settle = |runs: &str, open: Option<rolling_issue::Issue>| {
        let mut tracker = recorder(open);
        main_red::settle(&mut tracker, runs).unwrap();
        tracker
    };
    let fixed = settle(&run_list(&[("Cargo", "fix", "success"), ("Harness", "fix", "success")]), issue());
    assert_eq!(fixed.closed.len(), 1);
    assert_eq!(fixed.closed[0].0, 9);
    assert!(fixed.edited[0].1.contains("- [x] The newest Harness and Cargo runs on main are green"));
    assert!(fixed.closed[0].1.contains("https://runs/Cargo"));
    assert!(settle(&run_list(&[("Cargo", "tip", "success"), ("Harness", "fix", "success")]), issue()).closed.is_empty(), "a green rerun of the culprit is not a repair");
    assert!(settle(&run_list(&[("Cargo", "fix", "success"), ("Harness", "fix", "failure")]), issue()).closed.is_empty());
    assert!(settle(&run_list(&[("Cargo", "fix", "failure"), ("Cargo", "old", "success"), ("Harness", "fix", "success")]), issue()).closed.is_empty());
    assert!(settle(&run_list(&[("Cargo", "fix", "cancelled"), ("Cargo", "older", "success"), ("Harness", "fix", "success")]), issue()).closed.len() == 1, "a cancelled run is no verdict");
    assert!(settle(&run_list(&[("Main is red", "fix", "success")]), issue()).closed.is_empty(), "only Harness and Cargo decide");
    assert!(settle(&run_list(&[("Cargo", "fix", "success")]), issue()).closed.is_empty(), "Harness has not reported");
    assert!(settle(&run_list(&[("Main is red", "fix", "failure"), ("Canary", "fix", "failure"), ("Cargo", "fix", "success"), ("Harness", "fix", "success")]), issue()).closed.len() == 1);
    assert!(settle(&run_list(&[("Cargo", "fix", "success")]), None).closed.is_empty());
    assert!(settle("[]", issue()).closed.is_empty());
}

const LOOP_DOC: &str = "# Feedback loop\n\n| signal | writer | reader on main | verdict | commit |\n|---|---|---|---|---|\n| a | w | src/live.rs pick_tier; src/live.rs:2 | read | |\n| b | w | src/live.rs:9 | read | |\n| c | w | src/gone.rs | read | |\n| d | w | src/live.rs missing_name | read | |\n| e | w | none | dropped | |\n\n## File signals\n\n| signal file | reader stamp |\n|---|---|\n| `agents/fed.json` | `north/fed-stamp` |\n| `agents/starved.json` | `north/starved-stamp` |\n| `agents/opaque.json` | unverifiable |\n";

#[test]
fn health_loops_flags_reader_cells_that_no_longer_resolve() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/live.rs"), "fn pick_tier() {}\nlet x = 1;\n").unwrap();
    let faults = loops::stale_readers(dir.path(), LOOP_DOC);
    assert_eq!(
        faults,
        vec![
            "b: src/live.rs:9 is past the end (2 lines)".to_owned(),
            "c: src/gone.rs does not exist".to_owned(),
            "d: missing_name is not in src/live.rs".to_owned(),
        ]
    );
}

#[test]
fn health_loops_flags_written_but_unread_signal_files() {
    let day = 24 * 60 * 60;
    let times = BTreeMap::from([
        ("agents/fed.json", 100 * day),
        ("north/fed-stamp", 99 * day),
        ("agents/starved.json", 100 * day),
        ("north/starved-stamp", 92 * day),
        ("agents/opaque.json", 100 * day),
    ]);
    let report = loops::unread_signals(LOOP_DOC, &|path| times.get(path).copied());
    assert_eq!(report.stale, vec!["agents/starved.json: written 8 days after its reader's north/starved-stamp".to_owned()]);
    assert_eq!(report.unverifiable, vec!["agents/opaque.json".to_owned()]);
    let missing_stamp = loops::unread_signals(LOOP_DOC, &|path| (path == "agents/fed.json").then_some(100 * day));
    assert_eq!(missing_stamp.stale.len(), 1);
}

#[test]
fn health_rework_prints_each_repository_rate_and_fails_above_the_alarm() {
    let rates = BTreeMap::from([("north".to_owned(), (1, 10)), ("wisp".to_owned(), (3, 10))]);
    let (line, result) = rework::verdict(&rates, 20.0);
    assert_eq!(line, "north 1/10 10%, wisp 3/10 30%");
    assert_eq!(result.unwrap_err(), "7-day rework rate over 20%: wisp 30%");
    assert!(rework::verdict(&rates, 30.0).1.is_ok());
    assert_eq!(rework::verdict(&BTreeMap::new(), 20.0), ("no landings on main in 7 days".to_owned(), Ok(())));
}

fn journal_line(fields: serde_json::Value) -> String {
    format!("{fields}\n")
}

fn failed_start(unit: &str) -> String {
    journal_line(json!({"MESSAGE": format!("Failed to start {unit}: a description."), "USER_UNIT": unit, "_SYSTEMD_USER_UNIT": "init.scope", "SYSLOG_IDENTIFIER": "systemd"}))
}

fn unit_output(unit: &str, message: &str) -> String {
    journal_line(json!({"MESSAGE": message, "_SYSTEMD_USER_UNIT": unit, "SYSLOG_IDENTIFIER": "north"}))
}

fn hour_of_failures() -> String {
    let mut journal = String::new();
    for _ in 0..3 {
        journal += &unit_output("north-watch.service", "error: unrecognized command 'watch --once'");
        journal += &unit_output("north-watch.service", "Run 'north' to open the TUI, or 'north --help' for help.");
        journal += &failed_start("north-watch.service");
    }
    for _ in 0..2 {
        journal += &failed_start("fleet-tick.service");
    }
    journal += "not json\n";
    journal += &unit_output("fleet-tick.service", "error: everything is fine");
    journal
}

#[test]
fn unit_failures_name_units_with_three_failed_starts_and_their_last_error_line() {
    let failing = failed_starts::scan(&hour_of_failures());
    assert_eq!(
        failing,
        [failed_starts::Failing { unit: "north-watch.service".into(), count: 3, error: "error: unrecognized command 'watch --once'".into() }]
    );
    let verdict = failed_starts::verdict(&failing).unwrap_err();
    assert!(verdict.contains("north-watch.service failed to start 3 times in the last hour: error: unrecognized command"));
    assert!(failed_starts::verdict(&[]).is_ok());
}

#[test]
fn a_unit_with_no_error_word_reports_its_last_output_then_its_start_message() {
    let mut journal = unit_output("a.service", "first");
    journal += &unit_output("a.service", "second");
    let mut bare = String::new();
    for _ in 0..3 {
        bare += &failed_start("b.service");
    }
    for _ in 0..3 {
        journal += &failed_start("a.service");
    }
    let errors: Vec<String> = failed_starts::scan(&(journal + &bare)).into_iter().map(|one| one.error).collect();
    assert_eq!(errors, ["second", "Failed to start b.service: a description."]);
}

#[test]
fn unit_failures_reach_the_proxy_once_per_unit_per_distinct_error_per_day() {
    let directory = tempfile::tempdir().unwrap();
    let state_file = directory.path().join("north/unit-failures.json");
    let journal = hour_of_failures();
    let mut sent: Vec<String> = Vec::new();
    let mut run = |now: u64, journal: &str, dry_run: bool| {
        let text = journal.to_owned();
        failed_starts::check(dry_run, now, &state_file, &|| Ok(text.clone()), &mut |message| {
            sent.push(message.to_owned());
            true
        })
    };
    let day = 20_000 * 86_400;
    assert!(run(day + 10, &journal, true).is_err());
    assert!(run(day + 20, &journal, false).is_err());
    assert!(run(day + 30, &journal, false).is_err());
    assert!(run(day + 40, &journal.replace("watch --once", "watch --twice"), false).is_err());
    assert!(run(day + 50, &journal.replace("north-watch", "other"), false).is_err());
    assert!(run(day + 86_400, &journal, false).is_err());
    assert!(run(day + 86_500, &"{}\n".repeat(2), false).is_ok());
    assert_eq!(sent.len(), 4);
    assert!(sent[0].starts_with("harness unit north-watch.service failed to start 3 times in the last hour: error: unrecognized command 'watch --once'"));
    assert!(sent[1].contains("watch --twice"));
    assert!(sent[2].contains("other.service"));
    assert_eq!(sent[3], sent[0]);
}

#[test]
fn a_failed_proxy_message_is_retried_on_the_next_run() {
    let directory = tempfile::tempdir().unwrap();
    let state_file = directory.path().join("unit-failures.json");
    let journal = hour_of_failures();
    let mut attempts = 0;
    for delivered in [false, true, true] {
        let _ = failed_starts::check(false, 1_000, &state_file, &|| Ok(journal.clone()), &mut |_| {
            attempts += 1;
            delivered
        });
    }
    assert_eq!(attempts, 2);
}

#[test]
fn an_unreadable_journal_is_reported_not_swallowed() {
    let directory = tempfile::tempdir().unwrap();
    let error = failed_starts::check(false, 1, &directory.path().join("state.json"), &|| Err("journalctl exited 1".into()), &mut |_| true).unwrap_err();
    assert_eq!(error, "journalctl exited 1");
}
