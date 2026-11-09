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
        pass(host, Some(&cfg), paused, state.path(), now, || {
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
    assert!(matches!(pass("whiterabbit", Some(&unset), None, state.path(), 1_300.0, || 0), Pass::Elsewhere(_)), "a config naming no dispatch host runs dispatch nowhere");
    assert_eq!(tick("whiterabbit", None, 1_300.0), Pass::Ran(0));
    assert_eq!(runs.get(), 2, "five minutes after a run the tick runs dispatch again");
    assert_eq!(tick("whiterabbit", None, 1_300.5), Pass::Recent);
}
