#![cfg(test)]

use super::build_offload::*;

// KEEP(contract): a lane with a private game asset (.W3X, .blp) never ships to an offload host, and an oversized diff stays local; mutant bo-private-ships
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
    Candidate { class, landing: false, need: None, deferred, agent, offloaded_already: false }
}

// KEEP(contract): a fleet session's cargo check and clippy (moderate) are offered to build hosts like its builds, while another shell's admitted moderate build stays local; north#634
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

// KEEP(contract): only a shortage of hosts makes a fleet session's build refused on the laptop; a pinned local need or a landing runs here; north#634
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

// KEEP(contract): a fleet session's compiles go to build hosts, while Tom's shell, a landing, a local need and a run already on a build host keep today's path; north#634
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

// KEEP(contract): the gate's absolute path to the laptop's real cargo reaches the build host as the bare program name; north#634
#[test]
fn capacity_a_laptop_toolchain_path_is_sent_to_the_build_host_as_the_program_name() {
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(for_build_host(&words("/nix/store/abc-rust/bin/cargo test -p x")), words("cargo test -p x"));
    assert_eq!(for_build_host(&words("/nix/store/abc/bin/cargo-nextest nextest run")), words("cargo-nextest nextest run"));
    assert_eq!(for_build_host(&words("/home/tom/bin/tool run")), words("/home/tom/bin/tool run"));
    assert_eq!(for_build_host(&words("bash -c cargo")), words("bash -c cargo"));
    assert!(for_build_host(&[]).is_empty());
}

// KEEP(contract): a command whose path or redirect the agent can change is refused rather than compiled on the laptop, and only Rust programs are refused for want of a host; north#634
#[test]
fn capacity_a_fixable_command_is_kept_off_the_laptop_and_only_rust_programs_are_refused_for_want_of_a_host() {
    assert!(Declined::refused("absolute path", String::new()).lacks_host() && Declined::refused("laptop redirect", String::new()).lacks_host());
    assert!(!Declined::refused("private asset", String::new()).lacks_host() && !Declined::refused("oversized diff", String::new()).lacks_host());
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert!(is_rust_command(&words("cargo test")) && is_rust_command(&words("/nix/store/x/bin/rustc a.rs")) && is_rust_command(&words("cargo-nextest nextest run")));
    assert!(!is_rust_command(&words("bun test")) && !is_rust_command(&words("nix build")) && !is_rust_command(&[]));
}

