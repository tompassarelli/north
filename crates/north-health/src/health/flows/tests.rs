use super::*;
use crate::foundation::clock::Timestamp;

fn at(text: &str) -> f64 {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds_f64).unwrap()
}

fn host(name: &str, now: f64) -> Host {
    let facts = HostFacts { host: name.into(), runtime: Some("a".repeat(40)), policy_published: Some(now - 3600.0), newest_activity: Some(now - 60.0), ..HostFacts::default() };
    Host { name: name.into(), facts: Ok(facts), behind_since: Ok(None) }
}

/// The fleet as it should be: nexus records runs and runs dispatch, the laptop reads a fresh snapshot.
fn healthy(now: f64) -> World {
    let mut nexus = host("nexus", now);
    let facts = nexus.facts.as_mut().unwrap();
    facts.newest_run = Some(now - 120.0);
    facts.dispatch = Some(Dispatch { timer_active: true, paused: None, last_run: Some(now - 30.0) });
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
    facts(&mut world, "nexus").newest_run = Some(at("2026-11-07T01:23:00Z"));
    facts(&mut world, "whiterabbit").newest_activity = Some(at("2026-11-07T01:35:00Z"));
    alarms_only(&world, "run-recording", "newest recorded run ended 13 min ago");

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
    facts(&mut world, "nexus").dispatch = Some(Dispatch { timer_active: false, paused: None, last_run: Some(at("2026-11-07T02:56:00Z")) });
    alarms_only(&world, "dispatch", "nexus has its dispatch timer stopped with no pause recorded");

    let mut world = healthy(at("2026-11-07T03:01:00Z"));
    let paused = Held { since: at("2026-11-07T02:57:00Z"), cause: "dispatch staffs issues the proxy is replanning".into() };
    facts(&mut world, "nexus").dispatch = Some(Dispatch { timer_active: true, paused: Some(paused), last_run: Some(at("2026-11-07T02:56:00Z")) });
    alarms_only(&world, "dispatch", "nexus runs the dispatch timer though the proxy paused dispatch 4 min ago");
}
