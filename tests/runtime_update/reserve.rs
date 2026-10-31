use crate::harness::*;
use std::fs;
use std::path::Path;
use std::process::Output;

const ROOM_PROBE: &str = "20\n1.50 1.00 1.00 2/900 1\nMemTotal:       65536000 kB\nMemAvailable:   40000000 kB\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/md1 490000000 400000000 50000000 90% /\n@ADMISSION@\n{\"profile\":\"unattended\",\"aggregateCpuLimit\":20,\"committedBatchCpus\":2,\"cpuSomeAvg10\":5}\n";

pub fn reserved_puller(host: &Host, peer_built: &Path) -> Vec<(&'static str, String)> {
    host.built(PREVIOUS);
    host.select(PREVIOUS);
    script(
        &host.path("bin/rsync"),
        &format!("[[ -f {} ]] || exit 23\nfor program in north verify-north; do printf '#!/bin/sh\nexit 0\n'>\"${{@: -1}}/$program\"; chmod +x \"${{@: -1}}/$program\"; done\n", peer_built.display()),
    );
    script(&host.path("bin/nix-store"), "exit 0\n");
    script(&host.path("bin/ssh"), &format!("case \"$*\" in *nproc*) printf '{probe}'; exit 0;; esac\nexit 0\n", probe = ROOM_PROBE.replace('%', "%%")));
    let orchestration = fleet(host, "[hosts.builder]\nssh = \"tom@builder\"\n");
    fs::write(host.path("game-reserve.json"), "{\"on\":true,\"since\":99999999999}").unwrap();
    vec![
        ("AGENTS_ORCHESTRATION", orchestration.display().to_string()),
        ("RESIDENT_CAP_HOST", "puller".to_owned()),
        ("GAME_RESERVE_STATE", host.path("game-reserve.json").display().to_string()),
    ]
}

pub fn tick(host: &Host, environment: &[(&'static str, String)], sync_status: u8) -> Output {
    let borrowed: Vec<(&str, &str)> = environment.iter().map(|(key, value)| (*key, value.as_str())).collect();
    host.run(&["runtime", "update"], sync_status, 0, &borrowed)
}

#[test]
fn a_peer_runtime_not_yet_written_while_in_game_reserve_is_retried_next_tick_and_never_marked_failed() {
    let host = Host::new();
    let peer_built = host.path("peer-built");
    let environment = reserved_puller(&host, &peer_built);
    let first = tick(&host, &environment, 0);
    assert_eq!(first.status.code(), Some(75), "{}", String::from_utf8_lossy(&first.stderr));
    assert!(String::from_utf8_lossy(&first.stdout).contains("on builder while this host is in game-reserve"), "{}", String::from_utf8_lossy(&first.stdout));
    assert!(!host.path("state/north/runtime-failed").exists());
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
    fs::write(&peer_built, "").unwrap();
    let second = tick(&host, &environment, 0);
    assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
    assert!(!host.path("state/north/runtime-failed").exists());
}

#[test]
fn a_game_reserve_host_that_no_other_host_has_room_for_builds_the_runtime_itself_under_local_admission() {
    let host = Host::new();
    let peer_built = host.path("peer-built");
    let environment = reserved_puller(&host, &peer_built);
    script(&host.path("bin/ssh"), "exit 255\n");
    let skill = host.path("skill/machine-capacity");
    script(&host.path("bin/agents"), &format!("case \"$1\" in path) echo {};; sync) exit 0;; esac\n", skill.display()));
    script(
        &host.path("bin/bun"),
        "release=\"$HOME/.cache/cargo-target/north/release\"\nmkdir -p \"$release\"\nfor program in north verify-north; do printf '#!/bin/sh\\nexit 0\\n' >\"$release/$program\"; chmod +x \"$release/$program\"; done\n",
    );
    let output = tick(&host, &environment, 0);
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("building"), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(host.current(), Path::new("runtimes").join(&host.target));
}

#[test]
fn a_pulled_runtime_that_fails_its_sync_is_still_marked_failed_and_held_back() {
    let host = Host::new();
    let peer_built = host.path("peer-built");
    fs::write(&peer_built, "").unwrap();
    let environment = reserved_puller(&host, &peer_built);
    let output = tick(&host, &environment, 42);
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_to_string(host.path("state/north/runtime-failed")).unwrap(), host.target);
    assert_eq!(host.current(), Path::new("runtimes").join(PREVIOUS));
}

