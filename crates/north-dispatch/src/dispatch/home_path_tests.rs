//! north#1054: dispatch places a box whose text names a file under this host's home where that file is readable, or skips it for the round.
use super::decide;
use super::slots::Account;
use super::tests::{host, issue, sized, survey, MAX_5X};

fn account(name: &str, hosts: &[&str]) -> Account {
    sized(name, hosts, MAX_5X, super::slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 })
}

/// On 2026-11-10 at 3:49 PM all four free gex45 slots went to north#722 boxes naming /home/tom/code/north/worktrees/n722-gate-debt, which `north fleet launch` refused, so eight ready boxes launched nothing.
#[test]
fn a_box_naming_a_file_under_this_hosts_home_is_skipped_on_a_remote_host_and_the_slot_goes_to_the_next_box() {
    let accounts = vec![account("claude:a", &["nexus"]), account("claude:b", &["gex45"])];
    let named = issue("north", 722, "- [ ] read /home/tom/code/north/worktrees/n722-gate-debt/notes.md and fix the gate", &["priority:next"]);
    let neutral = issue("north", 723, "- [ ] the gate counts debt", &[]);
    let round = |hosts| decide(&survey(vec![named.clone(), neutral.clone()], hosts, accounts.clone()), "test");

    let remote = round(vec![host("gex45", 1, Ok(0))]);
    let launched: Vec<String> = remote.launches.iter().map(|launch| format!("{} on {}", launch.item, launch.host)).collect();
    assert_eq!(launched, ["north#723 on gex45"], "{:?}", remote.skipped);
    assert!(remote.skipped.iter().any(|(item, why)| item.to_string() == "north#722" && why.contains("/home/tom/code/north/worktrees/n722-gate-debt/notes.md") && why.contains("gex45")), "{:?}", remote.skipped);

    let both = round(vec![host("gex45", 1, Ok(0)), host("nexus", 1, Ok(0))]);
    let mut placed: Vec<String> = both.launches.iter().map(|launch| format!("{} on {}", launch.item, launch.host)).collect();
    placed.sort();
    assert_eq!(placed, ["north#722 on nexus", "north#723 on gex45"], "a box that names a home file runs where it is readable: {:?}", both.skipped);
}

/// On 2026-11-10 at 6:38 PM a dry run left over a dozen ready boxes unplaced because their text named /home/tom/code/north/main, which cpx62 holds as every fleet host does.
#[test]
fn a_box_naming_a_primary_checkout_places_on_a_remote_fleet_host() {
    let accounts = vec![account("claude:a", &["cpx62"])];
    let named = issue("north", 1020, "- [ ] read /home/tom/code/north/main/docs/rationale/dispatch-spread.md and fix it", &[]);
    let round = decide(&survey(vec![named], vec![host("cpx62", 1, Ok(0))], accounts), "test");
    let launched: Vec<String> = round.launches.iter().map(|launch| format!("{} on {}", launch.item, launch.host)).collect();
    assert_eq!(launched, ["north#1020 on cpx62"], "{:?}", round.skipped);
}

/// On 2026-11-10 north#1041's box naming `$XDG_STATE_HOME/north/gates/peer-invocation.jsonl` was staffed on a host that cannot read it, and stopped on the leash.
#[test]
fn a_box_naming_a_state_file_by_environment_variable_is_skipped_on_a_remote_host() {
    let accounts = vec![account("claude:a", &["gex45"])];
    let named = issue("north", 1041, "- [ ] a reviewer reads `$XDG_STATE_HOME/north/gates/peer-invocation.jsonl` and fixes any false refusal", &[]);
    let round = decide(&survey(vec![named], vec![host("gex45", 1, Ok(0))], accounts), "test");
    assert!(round.launches.is_empty(), "{:?}", round.launches);
    assert!(round.skipped.iter().any(|(_, why)| why.contains("/.local/state/north/gates/peer-invocation.jsonl")), "{:?}", round.skipped);
}
