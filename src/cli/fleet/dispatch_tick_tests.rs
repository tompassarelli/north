use super::*;

fn config(host: &str) -> DocumentMut {
    format!("[placement]\ndispatch_host = \"{host}\"\n").parse().unwrap()
}

#[test]
fn the_fleet_tick_runs_dispatch_when_it_is_not_paused() {
    let hosts: DocumentMut = "[hosts.nexus]\ndeploy_ssh = \"nexus\"\n[hosts.gex45]\nssh = \"gex45\"\n[hosts.laptop]\nssh = \"laptop\"\n[hosts.cloud]\nresident_cap = 2\n[hosts.odd]\nssh = \"-oProxyCommand=x\"\n".parse().unwrap();
    let pulled: Vec<String> = super::super::api_spend::far_hosts(&hosts, "laptop").into_iter().map(|(name, _)| name).collect();
    assert_eq!(pulled, ["nexus", "gex45"]);

    let state = tempfile::tempdir().unwrap();
    let cfg = config("whiterabbit");
    let runs = std::cell::Cell::new(0);
    let tick = |host: &str, paused: Option<&str>, now: f64| {
        pass(host, Some(&cfg), paused, state.path(), now, || Some(3), || {
            runs.set(runs.get() + 1);
            0
        })
    };

    assert_eq!(tick("whiterabbit", None, 1_000.0), Pass::Ran(0));
    assert_eq!(runs.get(), 1, "the dispatch host's tick runs dispatch");
    assert_eq!(tick("whiterabbit", None, 1_060.0), Pass::Recent);
    assert_eq!(runs.get(), 1, "a tick within five minutes of a run starts no second one");
    assert_eq!(tick("nexus", None, 1_300.0), Pass::Elsewhere("this host is not the dispatch host, whiterabbit".into()));
    assert_eq!(tick("whiterabbit", Some("account e is for landing"), 1_300.0), Pass::Paused("account e is for landing".into()));
    assert_eq!(runs.get(), 1, "neither a host that does not own dispatch nor a paused one runs it");
    let unset: DocumentMut = "[placement]\n".parse().unwrap();
    assert!(matches!(pass("whiterabbit", Some(&unset), None, state.path(), 1_300.0, || Some(3), || 0), Pass::Elsewhere(_)), "a config naming no dispatch host runs dispatch nowhere");
    assert_eq!(tick("whiterabbit", None, 1_300.0), Pass::Ran(0));
    assert_eq!(runs.get(), 2, "five minutes after a run the tick runs dispatch again");
    assert_eq!(tick("whiterabbit", None, 1_300.5), Pass::Recent);
}

#[test]
fn a_session_that_frees_a_slot_runs_dispatch_on_the_next_tick_not_the_next_five_minutes_north_1040() {
    let cfg = config("whiterabbit");
    let wait_after_drop = |sees_the_drop: bool, dropped_at: f64| {
        let state = tempfile::tempdir().unwrap();
        let count = std::cell::Cell::new(3);
        let tick = |now: f64| {
            let resident = || Some(if sees_the_drop { count.get() } else { 3 });
            matches!(pass("whiterabbit", Some(&cfg), None, state.path(), now, resident, || 0), Pass::Ran(_))
        };
        assert!(tick(0.0));
        let mut now = 0.0;
        loop {
            now += 30.0;
            if now >= dropped_at {
                count.set(2);
            }
            if tick(now) && now >= dropped_at {
                return now - dropped_at;
            }
        }
    };
    let drops = [10.0, 45.0, 100.0, 160.0, 220.0, 280.0];
    let before: Vec<f64> = drops.iter().map(|at| wait_after_drop(false, *at)).collect();
    let after: Vec<f64> = drops.iter().map(|at| wait_after_drop(true, *at)).collect();
    eprintln!("wait for dispatch after a slot frees, s: before {before:?}, after {after:?}");
    assert!(before.iter().copied().fold(0.0, f64::max) >= 200.0);
    assert!(after.iter().all(|wait| *wait <= 60.0), "{after:?}");
}
