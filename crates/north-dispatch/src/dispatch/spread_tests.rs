//! The dispatch-spread checks (north#259) the saturated-host tests run.
use super::decide;
use super::plan::Cloud;
use super::slots::{Account, Host};
use super::tests::{host, issue, survey};
use crate::capacity::saturation::{Backlog, Saturation};

fn account(name: &str, hosts: &[&str]) -> Account {
    let room = super::slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 };
    super::tests::sized(name, hosts, super::tests::MAX_5X, room)
}

fn reading(disk_free_gib: f64, disk_total_gib: f64, queued: u32) -> Saturation {
    Saturation { memory_total_mib: 65_536.0, memory_available_mib: 49_152.0, disk_free_gib, disk_total_gib, backlog: Some(Backlog { queued, load: 8.0, threads: 20 }), token_age_s: None }
}

fn placed(hosts: Vec<Host>, accounts: Vec<Account>, boxes: u64) -> (Vec<(String, String)>, Vec<String>) {
    let issues = (0..boxes).map(|number| issue("makgora", 100 + number, "- [ ] a", &[])).collect();
    let outcome = decide(&survey(issues, Cloud::Closed("no credit".into()), hosts, accounts), "test");
    let launched = outcome.launches.iter().map(|launch| (launch.host.clone(), launch.account.clone())).collect();
    (launched, outcome.skipped.into_iter().map(|(item, why)| format!("skip {item}: {why}")).collect())
}

pub(super) fn launches_earlier_in_the_run_count_against_each_hosts_sessions_and_admission_queue() {
    let accounts = vec![account("claude:a", &["nexus"]), account("claude:b", &["gex45"])];
    let (launched, skipped) = placed(vec![host("gex45", 8, Ok(5)), host("nexus", 6, Ok(4))], accounts.clone(), 7);
    let hosts: Vec<&str> = launched.iter().map(|(host, _)| host.as_str()).collect();
    assert_eq!(hosts, ["gex45", "gex45", "gex45", "nexus", "nexus"]);
    assert!(skipped[0].contains("gex45 has no free slot") && skipped[0].contains("nexus has no free slot"), "{}", skipped[0]);

    let queued = host("gex45", 24, Ok(0)).read(Some(&reading(400.0, 936.0, 1)));
    assert_eq!(queued.builds, Some(3));
    let (launched, skipped) = placed(vec![queued, host("nexus", 6, Ok(0))], accounts, 17);
    let hosts: Vec<&str> = launched.iter().map(|(host, _)| host.as_str()).collect();
    assert_eq!(hosts, ["gex45", "gex45", "gex45", "nexus", "nexus", "nexus", "nexus", "nexus", "nexus"]);
    assert!(skipped[0].contains("gex45 admission queue has no room for another build this run"), "{}", skipped[0]);
}

pub(super) fn a_host_whose_admission_queue_is_at_its_limit_or_whose_root_disk_is_past_the_reapers_eviction_line_takes_no_session() {
    let accounts = vec![account("claude:a", &["nexus"]), account("claude:b", &["gex45"])];
    let full_disk = host("gex45", 24, Ok(0)).read(Some(&reading(46.8, 936.0, 0)));
    assert_eq!(full_disk.free(), Err("gex45 skipped: 46.8 GiB free on the root disk, under the 94 GiB line the lease reaper evicts offload slots below".to_owned()));
    let queue = host("gex45", 24, Ok(0)).read(Some(&reading(400.0, 936.0, 4)));
    assert_eq!(queue.free(), Err("gex45 skipped: 4 jobs queued in machine-capacity admission (limit 4)".to_owned()));
    assert_eq!(host("gex45", 24, Ok(0)).read(Some(&reading(123.0, 936.0, 0))).free(), Ok(24), "87% full with 123 GiB free is over the eviction line, so the reaper sleeps and placement admits");

    for refused in [full_disk, queue] {
        let why = refused.free().unwrap_err();
        let (launched, skipped) = placed(vec![refused, host("nexus", 6, Ok(5))], accounts.clone(), 2);
        assert_eq!(launched, [("nexus".to_owned(), "claude:a".to_owned())]);
        assert!(skipped[0].contains(&format!("gex45 takes nothing: {why}")), "{}", skipped[0]);
    }
}

pub(super) fn a_host_whose_pushed_github_token_is_past_its_trust_takes_no_session_and_its_box_places_on_another_host() {
    let accounts = vec![account("claude:a", &["nexus"]), account("claude:b", &["gex45"])];
    let aged = |minutes: u64| host("gex45", 24, Ok(0)).read(Some(&Saturation { token_age_s: Some(minutes * 60), ..reading(400.0, 936.0, 0) }));
    assert_eq!(aged(54).free(), Ok(24), "a token inside its 55 min trust refuses nothing");
    let stale = aged(56);
    assert_eq!(stale.free(), Err("gex45 skipped: its GitHub token is 56 min old, past its 55 min trust, so a worker there cannot reach GitHub".to_owned()));
    let (launched, skipped) = placed(vec![stale, host("nexus", 6, Ok(0))], accounts, 1);
    assert_eq!(launched, [("nexus".to_owned(), "claude:a".to_owned())]);
    assert!(skipped.is_empty(), "{skipped:?}");
    let probed = crate::capacity::saturation::parse("65536000\n49152000\n/dev/md1 981000000 400000000 500000000 45% /\n#backlog\n0 1.0 20\n#token\n3400\n").unwrap();
    assert_eq!(probed.token_age_s, Some(3400));
    assert_eq!(crate::capacity::saturation::parse("65536000\n49152000\n/dev/md1 981000000 400000000 500000000 45% /\n#backlog\n#token\n").unwrap().token_age_s, None, "a host that mints its own token reports none");
}
