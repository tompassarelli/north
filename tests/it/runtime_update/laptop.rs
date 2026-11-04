use super::harness::*;
use super::reserve::{ROOM_PROBE, tick};
use std::fs;
use std::path::Path;

const GLIBC: &str = "/nix/store/n51dhmdbik1kfrsm62j5knavmigwrl1a-glibc-2.42-84";

fn builder_offers(host: &Host) {
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(
        &host.path("bin/rsync"),
        &format!("printf 'rsync %s\\n' \"$*\" >>\"$TEST_ALERTS\"\nfor program in north verify-north; do printf '#!/bin/sh\\n# {GLIBC}\\nexit 0\\n' >\"${{@: -1}}/$program\" 2>/dev/null; chmod +x \"${{@: -1}}/$program\" 2>/dev/null; done\nexit 0\n"),
    );
    script(&host.path("bin/nix-store"), "printf 'nix-store %s\\n' \"$*\" >>\"$TEST_ALERTS\"\n");
    script(&host.path("bin/ssh"), &format!("case \"$*\" in *nproc*) printf '{probe}'; exit 0;; esac\nprintf 'ssh %s\\n' \"$*\" >>\"$TEST_ALERTS\"\nexit 0\n", probe = ROOM_PROBE.replace('%', "%%")));
    let skill = host.path("skill/machine-capacity");
    script(&host.path("bin/agents"), &format!("case \"$1\" in path) echo {};; sync) exit 0;; esac\n", skill.display()));
    script(
        &host.path("bin/bun"),
        "release=\"$HOME/.cache/cargo-target/north/release\"\nmkdir -p \"$release\"\nfor program in north verify-north; do printf '#!/bin/sh\\nexit 0\\n' >\"$release/$program\"; chmod +x \"$release/$program\"; done\nprintf 'local build\\n' >>\"$TEST_ALERTS\"\n",
    );
}

fn pulled_from_builder(host: &Host) -> bool {
    let calls = fs::read_to_string(host.path("alerts")).unwrap_or_default();
    calls.contains(&format!("tom@builder:.local/state/north/runtimes/{}/north", host.target))
}

fn environment(host: &Host, name: &str, hosts: &str, reserved: bool) -> Vec<(&'static str, String)> {
    let orchestration = fleet(host, hosts);
    let mut environment = vec![("AGENTS_ORCHESTRATION", orchestration.display().to_string()), ("RESIDENT_CAP_HOST", name.to_owned())];
    if reserved {
        fs::write(host.path("game-reserve.json"), "{\"on\":true,\"since\":99999999999}").unwrap();
        environment.push(("GAME_RESERVE_STATE", host.path("game-reserve.json").display().to_string()));
    }
    environment
}

const FLEET: &str = "[hosts.builder]\nssh = \"tom@builder\"\n[hosts.laptop]\nsettles_fleet = true\n";

#[test]
fn a_host_that_settles_the_fleet_builds_its_runtime_here_though_another_host_offers_one() {
    let host = Host::new();
    builder_offers(&host);
    let environment = environment(&host, "laptop", FLEET, false);
    let output = tick(&host, &environment, 0);
    let calls = fs::read_to_string(host.path("alerts")).unwrap_or_default();
    assert!(!pulled_from_builder(&host), "{calls}");
    assert!(calls.contains("local build"), "{calls}\n{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
}

#[test]
fn a_host_that_does_not_settle_the_fleet_still_pulls_what_the_settler_built() {
    let host = Host::new();
    builder_offers(&host);
    let environment = environment(&host, "builder-peer", FLEET, false);
    let output = tick(&host, &environment, 0);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(pulled_from_builder(&host));
    assert!(!fs::read_to_string(host.path("alerts")).unwrap().contains("local build"));
}

#[test]
fn a_host_that_settles_the_fleet_defers_its_build_in_game_reserve_and_keeps_the_live_runtime() {
    let host = Host::new();
    builder_offers(&host);
    let environment = environment(&host, "laptop", FLEET, true);
    let output = tick(&host, &environment, 0);
    let calls = fs::read_to_string(host.path("alerts")).unwrap_or_default();
    assert!(!pulled_from_builder(&host), "{calls}");
    assert!(!calls.contains("local build"), "{calls}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("waits for the reserve to end"), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!host.path("state/north/runtime-failed").exists());
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

#[test]
fn a_host_that_settles_the_fleet_builds_the_deferred_runtime_once_game_reserve_ends() {
    let host = Host::new();
    builder_offers(&host);
    let reserved = environment(&host, "laptop", FLEET, true);
    let held = tick(&host, &reserved, 0);
    assert!(String::from_utf8_lossy(&held.stderr).contains("waits for the reserve to end"), "{}", String::from_utf8_lossy(&held.stderr));
    assert!(!fs::read_to_string(host.path("alerts")).unwrap_or_default().contains("local build"));
    fs::write(host.path("game-reserve.json"), "{\"on\":false,\"since\":0}").unwrap();
    tick(&host, &reserved, 0);
    let calls = fs::read_to_string(host.path("alerts")).unwrap_or_default();
    assert_eq!(calls.matches("local build").count(), 1, "{calls}");
    assert!(!pulled_from_builder(&host), "{calls}");
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    assert!(host.runtime(&host.target).join("built-here").is_file());
}

#[test]
fn a_host_that_settles_the_fleet_ignores_a_runtime_directory_without_its_own_build_marker_and_rebuilds_it() {
    let host = Host::new();
    builder_offers(&host);
    host.built(&host.target);
    let environment = environment(&host, "laptop", FLEET, false);
    assert!(!host.runtime(&host.target).join("built-here").exists());
    tick(&host, &environment, 0);
    let calls = fs::read_to_string(host.path("alerts")).unwrap_or_default();
    assert_eq!(calls.matches("local build").count(), 1, "{calls}");
    assert!(host.runtime(&host.target).join("built-here").is_file());
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
}

#[test]
fn a_host_that_does_not_settle_the_fleet_accepts_a_runtime_directory_without_a_build_marker() {
    let host = Host::new();
    builder_offers(&host);
    host.built(&host.target);
    let environment = environment(&host, "builder-peer", FLEET, false);
    let output = tick(&host, &environment, 0);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!fs::read_to_string(host.path("alerts")).unwrap_or_default().contains("local build"));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
}

#[test]
fn a_host_that_settles_the_fleet_refuses_a_runtime_another_host_ships_and_promotes() {
    let host = Host::new();
    host.built(&host.target);
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    let environment = environment(&host, "laptop", FLEET, false);
    let borrowed: Vec<(&str, &str)> = environment.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let output = host.north_with(&["runtime", "update", "--sha", &host.target], &borrowed);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not promote"), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}
