//! The dispatch-spread checks (north#259) the saturated-host and week tests run.
use super::decide;
use super::plan::Cloud;
use super::slots::{Account, Host};
use super::survey::pace_workers;
use super::tests::{host, issue, survey};
use crate::capacity::saturation::{Backlog, Saturation};
use serde_json::json;

fn account(name: &str, hosts: &[&str], workers: Option<i64>) -> Account {
    let room = super::slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 };
    Account { workers, ..super::tests::sized(name, hosts, super::tests::MAX_5X, room) }
}

fn reading(disk_free_gib: f64, disk_total_gib: f64, queued: u32) -> Saturation {
    Saturation { memory_total_mib: 65_536.0, memory_available_mib: 49_152.0, disk_free_gib, disk_total_gib, backlog: Some(Backlog { queued, load: 8.0, threads: 20 }) }
}

fn placed(hosts: Vec<Host>, accounts: Vec<Account>, boxes: u64) -> (Vec<(String, String)>, Vec<String>) {
    let issues = (0..boxes).map(|number| issue("makgora", 100 + number, "- [ ] a", &[])).collect();
    let outcome = decide(&survey(issues, Cloud::Closed("no credit".into()), hosts, accounts), "test");
    let launched = outcome.launches.iter().map(|launch| (launch.host.clone(), launch.account.clone())).collect();
    (launched, outcome.skipped.into_iter().map(|(item, why)| format!("skip {item}: {why}")).collect())
}

pub(super) fn an_account_pace_does_not_hold_launches_no_more_than_its_allowed_workers_less_its_live_workers() {
    let unheld = json!({"allowed_workers": 2, "held": false});
    assert_eq!(pace_workers(&unheld, 0), Some(2));
    assert_eq!(pace_workers(&unheld, 1), Some(1));
    assert_eq!(pace_workers(&json!({"allowed_workers": 1, "held": true}), 4), Some(0));
    assert_eq!(pace_workers(&json!({"held": false}), 0), None);

    let gex45 = host("gex45", 24, Ok(0));
    let (launched, skipped) = placed(vec![gex45.clone()], vec![account("claude:b", &["gex45"], pace_workers(&unheld, 0))], 17);
    assert_eq!(launched, vec![("gex45".to_owned(), "claude:b".to_owned()); 2]);
    assert_eq!(skipped.len(), 15);
    assert!(skipped[0].contains("claude:b has no session left under pace's allowed_workers"), "{}", skipped[0]);

    let (launched, _) = placed(vec![gex45], vec![account("claude:b", &["gex45"], pace_workers(&unheld, 1))], 17);
    assert_eq!(launched.len(), 1);
}

pub(super) fn launches_earlier_in_the_run_count_against_each_hosts_sessions_and_admission_queue() {
    let accounts = vec![account("claude:a", &["nexus"], None), account("claude:b", &["gex45"], None)];
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

pub(super) fn a_host_whose_admission_queue_is_at_its_limit_or_whose_root_disk_is_over_85_percent_takes_no_session() {
    let accounts = vec![account("claude:a", &["nexus"], None), account("claude:b", &["gex45"], None)];
    let full_disk = host("gex45", 24, Ok(0)).read(Some(&reading(46.8, 936.0, 0)));
    assert_eq!(full_disk.free(), Err("gex45 skipped: root disk 95% full, over 85%".to_owned()));
    let queue = host("gex45", 24, Ok(0)).read(Some(&reading(400.0, 936.0, 4)));
    assert_eq!(queue.free(), Err("gex45 skipped: 4 jobs queued in machine-capacity admission (limit 4)".to_owned()));
    assert_eq!(host("gex45", 24, Ok(0)).read(Some(&reading(146.0, 936.0, 0))).free(), Ok(24));

    for refused in [full_disk, queue] {
        let why = refused.free().unwrap_err();
        let (launched, skipped) = placed(vec![refused, host("nexus", 6, Ok(5))], accounts.clone(), 2);
        assert_eq!(launched, [("nexus".to_owned(), "claude:a".to_owned())]);
        assert!(skipped[0].contains(&format!("gex45 takes nothing: {why}")), "{}", skipped[0]);
    }
}
