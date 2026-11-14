use north_base::dispatch_pause::held_by;
use north_health::health::flows::{Dispatch, Held, Host, HostFacts, World};

const DEPLOYED: &str = "# /home/tom/.config/systemd/user/north-dispatch.timer
[Install]
WantedBy=timers.target

[Timer]
OnCalendar=*:0/5
Persistent=true

[Unit]
ConditionPathExists=!%h/.local/state/north/dispatch-paused
Description=Run north fleet dispatch every five minutes
";

fn nexus(unit: &str) -> World {
    let paused = Held { since: 0.0, cause: "account b is the last headroom".into() };
    let dispatch = Dispatch { timer_active: false, deploy_restarts: !held_by(unit), paused: Some(paused), last_run: Some(0.0) };
    let facts = HostFacts { host: "nexus".into(), dispatch: Some(dispatch), ..HostFacts::default() };
    World { now: 60.0, hosts: vec![Host { name: "nexus".into(), facts: Ok(facts), behind_since: Ok(None) }], snapshot: None }
}

fn dispatch_verdict(world: &World) -> Result<(), String> {
    world.judge().into_iter().find(|flow| flow.name == "dispatch").unwrap().verdict
}

#[test]
fn a_recorded_dispatch_pause_survives_a_host_deploy() {
    assert!(held_by(DEPLOYED));
    assert_eq!(dispatch_verdict(&nexus(DEPLOYED)), Ok(()));

    let before = DEPLOYED.replace("ConditionPathExists=!%h/.local/state/north/dispatch-paused\n", "");
    assert!(!held_by(&before));
    let why = dispatch_verdict(&nexus(&before)).unwrap_err();
    assert!(why.contains("nexus's dispatch timer has no ConditionPathExists=!%h/.local/state/north/dispatch-paused, so a host deploy restarts it under a pause"), "{why}");
}
