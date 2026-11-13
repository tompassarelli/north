use super::*;
use crate::foundation::clock::Timestamp;

fn at(text: &str) -> f64 {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds_f64).unwrap()
}

fn host(name: &str, now: f64) -> Host {
    let facts = HostFacts { host: name.into(), runtime: Some("a".repeat(40)), policy_published: Some(now - 3600.0), newest_finished: Some(now - 60.0), ..HostFacts::default() };
    Host { name: name.into(), facts: Ok(facts), behind_since: Ok(None) }
}

/// The fleet as it should be: nexus records runs and runs dispatch, the laptop reads a fresh snapshot.
fn healthy(now: f64) -> World {
    let mut nexus = host("nexus", now);
    let facts = nexus.facts.as_mut().unwrap();
    facts.newest_run = Some(now - 120.0);
    facts.recording_interval_s = Some(15.0 * 60.0);
    facts.dispatch = Some(Dispatch { timer_active: true, deploy_restarts: false, paused: None, last_run: Some(now - 30.0) });
    World { now, hosts: vec![host("gex45", now), nexus, host("whiterabbit", now)], snapshot: Some(Snapshot { written: Some(now - 300.0), failure: None }) }
}

fn facts<'a>(world: &'a mut World, name: &str) -> &'a mut HostFacts {
    world.hosts.iter_mut().find(|host| host.name == name).unwrap().facts.as_mut().unwrap()
}

#[track_caller]
fn alarms_only(world: &World, flow: &str, cause: &str) {
    for judged in world.judge() {
        match &judged.verdict {
            Err(why) if judged.name == flow => assert!(why.contains(cause), "{flow}: {why}"),
            Ok(()) if judged.name == flow => panic!("{flow} raised no alarm"),
            Err(why) => panic!("{} alarmed too: {why}", judged.name),
            Ok(()) => {}
        }
    }
}

#[test]
fn runs_that_stop_reaching_the_ledger_alarm_while_workers_run_and_the_frozen_snapshot_alarms_too() {
    let now = at("2026-11-07T01:36:00Z");
    let mut world = healthy(now);
    facts(&mut world, "nexus").newest_run = Some(at("2026-11-07T01:16:00Z"));
    facts(&mut world, "whiterabbit").newest_finished = Some(at("2026-11-07T01:35:00Z"));
    assert!(world.judge().iter().all(|flow| flow.verdict.is_ok()), "a 19 min lag is inside the 15 min timer and its slack");
    facts(&mut world, "nexus").newest_run = Some(at("2026-11-07T01:10:00Z"));
    alarms_only(&world, "run-recording", "newest recorded run ended 26 min ago");
    facts(&mut world, "nexus").recording_interval_s = None;
    alarms_only(&world, "run-recording", "worker-ledger timer");

    let now = at("2026-11-07T02:00:00Z");
    let mut world = healthy(now);
    world.snapshot = Some(Snapshot { written: Some(at("2026-11-07T01:37:00Z")), failure: None });
    alarms_only(&world, "snapshot", "last written 23 min ago");
}

#[test]
fn a_policy_generation_held_on_every_host_alarms_with_the_gates_cause() {
    let now = at("2026-11-06T23:30:00Z");
    let mut world = healthy(now);
    let cause = "skills/shared/github/SKILL.md description is 1030 bytes, 6 over its 1024-byte cap";
    for name in ["gex45", "nexus", "whiterabbit"] {
        facts(&mut world, name).policy_held = Some(Held { since: at("2026-11-06T23:16:00Z"), cause: cause.into() });
    }
    alarms_only(&world, "policy", &format!("gex45 has published no policy generation for 14 min: {cause}"));
}

#[test]
fn one_host_without_a_current_policy_generation_is_named_among_hosts_that_publish() {
    let now = at("2026-11-12T01:30:00Z");
    let mut world = healthy(now);
    let policy = world.judge().into_iter().find(|flow| flow.name == "policy").unwrap();
    assert_eq!(policy.readings.len(), 3);
    assert!(policy.verdict.is_ok());
    facts(&mut world, "nexus").policy_published = Some(now - 3.0 * 3600.0 - 600.0);
    alarms_only(&world, "policy", "nexus published its last policy generation 3 h 10 min ago, 2 h 10 min before the newest host");
    facts(&mut world, "nexus").policy_published = None;
    alarms_only(&world, "policy", "nexus has published no policy generation");
}

#[test]
fn a_launchable_profile_without_settings_alarms_as_a_host_launching_without_hooks() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    std::fs::write(home.path().join(".claude/settings.json"), r#"{"hooks":{"PreToolUse":[]}}"#).unwrap();
    let api = home.path().join("profiles/claude-api");
    std::fs::create_dir_all(&api).unwrap();
    let policy = serde_json::json!({"accounts": {"claude": {"api_keys": {"api": {"gex45": {"path": api.to_string_lossy()}}}}}});
    let mut world = healthy(at("2026-11-07T02:00:00Z"));
    facts(&mut world, "gex45").unhooked = north_pace::usage::profiles::unhooked(&policy, "gex45", home.path());
    alarms_only(&world, "hooks", "gex45 launches without hooks in api: ");
}

#[test]
fn a_dispatch_timer_stopped_without_a_pause_and_one_restarted_under_a_pause_both_alarm() {
    let mut world = healthy(at("2026-11-07T02:58:00Z"));
    facts(&mut world, "nexus").dispatch = Some(Dispatch { timer_active: false, deploy_restarts: false, paused: None, last_run: Some(at("2026-11-07T02:56:00Z")) });
    alarms_only(&world, "dispatch", "nexus has its dispatch timer stopped with no pause recorded");

    let mut world = healthy(at("2026-11-07T03:01:00Z"));
    let paused = Held { since: at("2026-11-07T02:57:00Z"), cause: "dispatch staffs issues the proxy is replanning".into() };
    facts(&mut world, "nexus").dispatch = Some(Dispatch { timer_active: true, deploy_restarts: false, paused: Some(paused), last_run: Some(at("2026-11-07T02:56:00Z")) });
    alarms_only(&world, "dispatch", "nexus runs the dispatch timer though the proxy paused dispatch 4 min ago");
}

#[test]
fn a_state_whose_holder_died_alarms_as_a_stuck_state_machine_and_an_unreadable_store_alarms_too() {
    let mut world = healthy(at("2026-11-07T02:30:00Z"));
    let owner = north_base::lifecycle::Owner { host: "whiterabbit".into(), pid: 4242, started: 1, booted_ms: 1 };
    let stuck = north_base::lifecycle::Fault {
        machine: "snapshot-pull".into(),
        instance: "whiterabbit".into(),
        state: "pulling".into(),
        since_ms: Timestamp::parse_rfc3339("2026-11-07T00:55:00Z").unwrap().unix_millis(),
        kind: north_base::lifecycle::FaultKind::Orphaned { owner },
        cause: "the snapshot is 15 min old".into(),
    };
    facts(&mut world, "whiterabbit").lifecycles = Lifecycles::Read(vec![stuck]);
    alarms_only(&world, "state-machines", "whiterabbit: snapshot-pull/whiterabbit pulling for 95 min, held by pid 4242 on whiterabbit, which is gone");

    let mut world = healthy(at("2026-11-07T02:30:00Z"));
    facts(&mut world, "gex45").lifecycles = Lifecycles::Unreadable("database is locked".into());
    alarms_only(&world, "state-machines", "gex45: its lifecycle store is unreadable: database is locked");
}

fn session(fleet: &std::path::Path, name: &str, host_pid: Option<u32>, wrote: f64) {
    let generation = fleet.join(name).join("g1");
    std::fs::create_dir_all(&generation).unwrap();
    if let Some(pid) = host_pid {
        std::fs::write(fleet.join(name).join("host.pid"), pid.to_string()).unwrap();
    }
    let stream = std::fs::File::create(generation.join("stream.jsonl")).unwrap();
    stream.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(wrote)).unwrap();
}

/// north#963: a worker still running has no run to record, so its stream writes must not read as unrecorded work.
#[test]
fn a_running_worker_is_no_finished_work_for_the_recording_check() {
    let fleet = tempfile::tempdir().unwrap();
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    session(fleet.path(), "live", Some(std::process::id()), 3000.0);
    assert_eq!(facts::newest_finished(fleet.path()), None);
    session(fleet.path(), "done", Some(dead), 1000.0);
    session(fleet.path(), "retired", None, 2000.0);
    assert_eq!(facts::newest_finished(fleet.path()), Some(2000.0));

    let now = at("2026-11-08T18:00:00Z");
    let mut world = healthy(now);
    facts(&mut world, "nexus").newest_run = Some(now - 40.0 * 60.0);
    for name in ["gex45", "nexus", "whiterabbit"] {
        facts(&mut world, name).newest_finished = None;
    }
    assert!(world.judge().iter().all(|flow| flow.verdict.is_ok()), "no worker finished, one runs: quiet");
}

#[test]
fn a_hosts_refused_threads_spool_writes_are_one_finding_per_verb_and_its_set_aside_batches_page_on_their_own() {
    let at_ms = |text: &str| Timestamp::parse_rfc3339(text).unwrap().unix_millis();
    let refused = |instance: &str, verb: &str, cause: &str, since: &str| north_base::lifecycle::Fault {
        machine: "threads-spool".into(),
        instance: instance.into(),
        state: "refused".into(),
        since_ms: at_ms(since),
        kind: north_base::lifecycle::FaultKind::Overdue { due_ms: at_ms(since) },
        cause: format!("the ledger host will not run `{verb}`: {cause}"),
    };
    let mut world = healthy(at("2026-11-08T09:49:00Z"));
    facts(&mut world, "gex45").lifecycles = Lifecycles::Read(vec![
        refused("a.argv", "claim", "remote-serve: refused for gex45: command claim is not on the allowlist", "2026-11-08T09:15:00Z"),
        refused("b.argv", "claim", "the ledger host exited 126", "2026-11-07T05:15:00Z"),
        refused("c.argv", "claim", "remote-serve: refused for gex45: command claim is not on the allowlist", "2026-11-08T08:00:00Z"),
        refused("d.argv", "release", "remote-serve: refused for gex45: command release is not on the allowlist", "2026-11-08T09:00:00Z"),
        north_base::lifecycle::Fault { cause: format!("{} 3 sends in a row timed out with the outcome unknown; e.jsonl is kept and can be sent again", north_threads::threads::client::spool::SET_ASIDE), ..refused("e.jsonl", "", "", "2026-11-08T09:40:00Z") },
    ]);
    let judged = world.judge();
    let verdict = judged.iter().find(|flow| flow.name == "state-machines").unwrap().verdict.as_ref().unwrap_err();
    assert_eq!(verdict.matches("threads-spool writes").count(), 2, "{verdict}");
    assert!(verdict.contains("gex45: 3 threads-spool writes refused, oldest 28 h 34 min ago"), "{verdict}");
    assert!(verdict.contains("gex45: 1 threads-spool writes refused, oldest 49 min ago"), "{verdict}");
    assert!(verdict.contains("`claim`") && verdict.contains("`release`"), "{verdict}");
    assert!(verdict.contains("the ledger host will not run `claim`: remote-serve: refused for gex45"), "the newest cause names the reason: {verdict}");
    assert!(!verdict.contains("exited 126"), "a verb's entries share one line with its newest cause: {verdict}");
    assert!(!verdict.contains(".argv"), "{verdict}");
    assert!(!verdict.contains("set aside"), "a set-aside batch is its own finding: {verdict}");
    let aside = judged.iter().find(|flow| flow.name == "spool-set-aside").unwrap().verdict.as_ref().unwrap_err();
    assert!(aside.starts_with("gex45: 1 threads-spool batches set aside, oldest 9 min ago: set aside after 3 sends"), "{aside}");
}
