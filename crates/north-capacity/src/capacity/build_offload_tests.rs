#![cfg(test)]

use super::build_host_command::for_build_host;
use super::build_offload::*;

fn candidate<'a>(class: &'a str, agent: bool, deferred: bool) -> Candidate<'a> {
    Candidate { class, landing: false, need: None, deferred, agent, offloaded_already: false, artifact: false }
}

#[test]
fn offloaded_build_runs_in_depbox() {
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(for_build_host(&words("/nix/store/abc-rust/bin/cargo test -p x"), false), words("north depbox run -- cargo test -p x"));
    assert_eq!(for_build_host(&words("/home/tom/.local/bin/depbox run -- nix develop -c cargo test"), false), words("north depbox run -- nix develop -c cargo test"), "the laptop's depbox becomes the build host's, never two nested");
    assert_eq!(for_build_host(&words("/home/tom/code/north-data/runtimes/abc/north --worker-sandbox --program -- cargo build -p x"), false), words("north depbox run -- cargo build -p x"), "a sandboxed session's worker-domain wrapper names the laptop's runtime under ~/code, which refused every such offload as an absolute path");
    assert_eq!(for_build_host(&words("/home/tom/code/north-data/runtimes/abc/north --worker-sandbox --program --worker -- true"), false), words("north depbox run -- true"), "on 2026-11-15 the broker marked a worker session's capacity host run with --worker, which the wrapper match missed, so every laptop worker's heavy run was refused as naming /home/tom/code/ (north#1592)");
    assert_eq!(for_build_host(&words("north land checks --leased"), true), words("north land checks --leased"), "a landing's checks enter depbox through land-shell, so its runner stays unwrapped");
}

#[test]
fn capacity_a_fleet_session_build_on_the_saturated_laptop_goes_to_the_build_host_with_the_most_free_room() {
    let room = |limit: f64, committed: f64| {
        let status = serde_json::json!({ "profile": "unattended", "aggregateCpuLimit": limit, "committedBatchCpus": committed, "cpuSomeAvg10": 0.0 });
        let admission = Admission::from_status(&status).expect("a host status");
        admission.shortfall(6.0, 0.0).map_or(Room::Free { free_cpus: admission.free_cpus() }, Room::Short)
    };
    let (laptop_is_a_build_host, gex45_is_a_build_host) = (false, true);
    assert!(offers_first(true, laptop_is_a_build_host), "the laptop at load 22 of 24 offers its fleet sessions' builds to the build hosts before its own admission decides");
    assert!(!offers_first(true, gex45_is_a_build_host), "a build host decides on its own sessions' builds first");
    assert!(!offers_first(false, laptop_is_a_build_host), "a build outside a fleet session stays on the laptop");
    let build = candidate("heavy", true, false);
    let measured = || vec![("gex45".to_owned(), room(20.0, 6.3)), ("cpx62".to_owned(), room(16.0, 0.8))];
    assert_eq!(route(&build, measured), Route::Offload { host: "cpx62".to_owned() });
    let busy_cpx62 = || vec![("gex45".to_owned(), room(20.0, 6.3)), ("cpx62".to_owned(), room(16.0, 12.0))];
    assert_eq!(route(&build, busy_cpx62), Route::Offload { host: "gex45".to_owned() });
    let full = || vec![("gex45".to_owned(), room(20.0, 18.0)), ("cpx62".to_owned(), Room::Unreachable)];
    assert!(matches!(route(&build, full), Route::Declined(declined) if declined.lacks_host()), "with every build host full or out of reach the build is refused, never run on the laptop");
    let closed = || vec![("cpx62".to_owned(), Room::Closed), ("cpx62b".to_owned(), Room::Closed)];
    assert!(matches!(route(&build, closed), Route::Declined(declined) if declined.lacks_host() && declined.detail().contains("cpx62b: closed")), "every build host closed is a decline naming each, not the local route");
}
