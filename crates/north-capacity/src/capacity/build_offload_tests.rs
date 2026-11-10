#![cfg(test)]

use super::build_host_command::for_build_host;
use super::build_offload::*;

#[test]
fn capacity_a_lane_with_a_private_asset_or_an_oversized_diff_stays_on_this_host() {
    assert_eq!(ships(&["src/lib.rs", "Cargo.toml"], 1000), Ok(()));
    let private = ships(&["maps/arena.W3X"], 10).unwrap_err();
    assert_eq!((private.reason(), private.detail()), ("refused: private asset".to_owned(), "maps/arena.W3X".to_owned()));
    assert!(ships(&["assets/a.blp"], 10).is_err());
    let oversized = ships(&["src/lib.rs"], MAX_DIFF_BYTES + 1).unwrap_err();
    assert_eq!(oversized.reason(), "refused: oversized diff");
    assert!(oversized.detail().contains("limit"));
}

fn candidate<'a>(class: &'a str, agent: bool, deferred: bool) -> Candidate<'a> {
    Candidate { class, landing: false, need: None, deferred, agent, offloaded_already: false, artifact: false }
}

#[test]
fn capacity_an_agents_moderate_build_is_offered_to_the_build_hosts_and_anyone_elses_is_not() {
    let free = || vec![("ccx33".to_owned(), Room::Free { free_cpus: 8.0 })];
    assert_eq!(route(&candidate("moderate", true, false), free), Route::Offload { host: "ccx33".to_owned() });
    assert_eq!(route(&candidate("moderate", false, true), free), Route::Local);
    assert_eq!(route(&candidate("heavy", true, false), free), Route::Offload { host: "ccx33".to_owned() });
    assert_eq!(route(&candidate("heavy", false, false), free), Route::Local);
    let rooms = |free: [Room; 3]| ["laptop", "ccx33", "gex45"].into_iter().map(str::to_owned).zip(free).collect::<Vec<_>>();
    let deferred = candidate("heavy", true, true);
    let peer = |name: &str| Route::Offload { host: name.to_owned() };
    let short = || Room::Short("0.0 cores free of 20, needs 6".to_owned());
    assert_eq!(route(&deferred, || rooms([Room::Free { free_cpus: 8.0 }, Room::Free { free_cpus: 14.0 }, short()])), peer("ccx33"));
    assert_eq!(route(&deferred, || rooms([Room::Free { free_cpus: 8.0 }, Room::Free { free_cpus: 8.0 }, Room::Unreachable])), peer("laptop"));
    assert_eq!(route(&deferred, || rooms([Room::Unreachable, short(), Room::Free { free_cpus: 6.0 }])), peer("gex45"));
}

#[test]
fn capacity_only_missing_build_hosts_decline_a_build_the_laptop_must_not_compile() {
    let short = || vec![("gex45".to_owned(), Room::Short("0.0 cores free of 20, needs 6".to_owned())), ("ccx33".to_owned(), Room::Unreachable)];
    let declined = match route(&candidate("heavy", true, false), short) {
        Route::Declined(declined) => declined,
        other => panic!("{other:?}"),
    };
    assert!(declined.lacks_host());
    assert!(declined.detail().contains("gex45: 0.0 cores free") && declined.detail().contains("ccx33: unreachable"));
    assert!(!Declined::refused("gpu", String::new()).lacks_host());
    assert!(Declined::Unreachable(vec![]).lacks_host() && Declined::Failed(String::new()).lacks_host());
}

#[test]
fn capacity_only_a_fleet_session_without_a_local_need_counts_as_an_agent_build() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |name: &str| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned());
    assert!(is_agent_build(env(&[("NORTH_FLEET_SESSION", "n634-x")])));
    assert!(is_agent_build(env(&[("NORTH_FLEET_SCOPE", "north-session-1.scope")])));
    assert!(!is_agent_build(env(&[])));
    assert!(!is_agent_build(env(&[("NORTH_FLEET_SESSION", "")])));
    assert!(!is_agent_build(env(&[("NORTH_FLEET_SESSION", "n"), ("SAFE_PUSH_LAND_HEAD", "abc")])));
    assert!(!is_agent_build(env(&[("NORTH_FLEET_SESSION", "n"), ("AGENT_CAPACITY_NEED", "gpu")])));
    assert!(!is_agent_build(env(&[("NORTH_FLEET_SESSION", "n"), (REMOTE_MARK, "1")])));
}

#[test]
fn capacity_a_laptop_toolchain_path_is_sent_to_the_build_host_as_the_program_name() {
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    let landing = |text: &str| for_build_host(&words(text), true);
    assert_eq!(landing("/nix/store/abc-rust/bin/cargo test -p x"), words("cargo test -p x"));
    assert_eq!(landing("/nix/store/abc/bin/cargo-nextest nextest run"), words("cargo-nextest nextest run"));
    assert_eq!(landing("/home/tom/bin/tool run"), words("/home/tom/bin/tool run"));
    assert_eq!(landing("bash -c cargo"), words("bash -c cargo"));
    assert_eq!(landing("/home/tom/.local/bin/depbox run -- nix develop -c cargo test"), words("nix develop -c cargo test"));
    assert_eq!(landing("/home/tom/.local/state/north/current/north depbox run -- /nix/store/x/bin/cargo build"), words("cargo build"));
    assert!(for_build_host(&[], false).is_empty());
}

#[test]
fn offloaded_build_runs_in_depbox() {
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(for_build_host(&words("/nix/store/abc-rust/bin/cargo test -p x"), false), words("north depbox run -- cargo test -p x"));
    assert_eq!(for_build_host(&words("/home/tom/.local/bin/depbox run -- nix develop -c cargo test"), false), words("north depbox run -- nix develop -c cargo test"), "the laptop's depbox becomes the build host's, never two nested");
    assert_eq!(for_build_host(&words("/home/tom/code/north-data/runtimes/abc/north --worker-sandbox --program -- cargo build -p x"), false), words("north depbox run -- cargo build -p x"), "a sandboxed session's worker-domain wrapper names the laptop's runtime under ~/code, which refused every such offload as an absolute path");
    assert_eq!(for_build_host(&words("north land checks --leased"), true), words("north land checks --leased"), "a landing's checks enter depbox through land-shell, so its runner stays unwrapped");
}

#[test]
fn capacity_a_fixable_command_is_kept_off_the_laptop() {
    assert!(Declined::refused("absolute path", String::new()).lacks_host() && Declined::refused("laptop redirect", String::new()).lacks_host());
    assert!(!Declined::refused("private asset", String::new()).lacks_host() && !Declined::refused("oversized diff", String::new()).lacks_host());
}


#[test]
fn capacity_a_build_that_writes_the_callers_artifact_is_not_offloaded_and_does_not_count_as_lacking_a_host() {
    let free = || vec![("laptop".to_owned(), Room::Free { free_cpus: 8.0 })];
    let build = Candidate { artifact: true, ..candidate("heavy", true, true) };
    let declined = match route(&build, free) {
        Route::Declined(declined) => declined,
        other => panic!("{other:?}"),
    };
    assert_eq!(declined.reason(), "refused: artifact");
    assert!(!declined.lacks_host());
    assert_eq!(route(&candidate("heavy", true, true), free), Route::Offload { host: "laptop".to_owned() });
    let none = || vec![("laptop".to_owned(), Room::Unreachable)];
    assert!(matches!(route(&build, none), Route::Declined(declined) if declined.lacks_host() && declined.reason() == "unreachable"), "with no host in reach the build reports the lack as any other");
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
    assert!(matches!(route(&build, full), Route::Declined(declined) if declined.lacks_host()), "the laptop builds it only once every build host is full or out of reach");
}
