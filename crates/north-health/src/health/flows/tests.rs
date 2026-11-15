use super::*;
use north_foundation::foundation::clock::Timestamp;

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
