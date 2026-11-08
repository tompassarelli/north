use north_land_probes::RefusalRate;

use super::Suspects;
use crate::land::serve::refusals::Refusals;

#[test]
fn a_round_judged_on_this_host_checks_one_probe_however_many_slots_the_build_hosts_report() {
    let ids: Vec<String> = (1..=7).map(|lane| format!("lane-{lane}")).collect();
    let suspects = Suspects::default();
    assert_eq!(suspects.plan(&ids, &[], 7, RefusalRate::default(), false).len(), 7);
    assert_eq!(suspects.plan(&ids, &[], 7, RefusalRate::default(), true).len(), 1, "on 2026-11-08 at 7:06 AM the lander, falling back to this host, ran 8 probes of 7 lanes at once in place and drove the laptop to load 123 while main stood still for 18 minutes");
}

// KEEP(bug): in the 8:33 AM round of 2026-11-08 the lander read 8 free slots of 14 and planned 8 probes, each of which then sent about 7 offload jobs to the build hosts, so 56 jobs queued for the slots and each waited 28 to 154 s
#[test]
fn a_replay_of_the_833_am_round_plans_only_as_many_probes_as_the_free_slots_hold_at_the_jobs_each_probe_took() {
    use crate::land::probe_slots::{fitting, jobs_per_probe};
    let logs = tempfile::tempdir().unwrap();
    let probe = |jobs: usize| (0..jobs).map(|job| if job + 1 == jobs { "safe-push: offloading 15 head-north land check(s) to gex45 as one job\n".to_owned() } else { "safe-push: offloading to gex45\n".to_owned() }).collect::<String>();
    for (number, jobs) in [0, 7, 7, 7, 6, 0, 7, 0].into_iter().enumerate() {
        let text = if jobs == 0 { "safe-push: verdict refused\n".to_owned() } else { probe(jobs) };
        std::fs::write(logs.path().join(format!("1794101633661375-1794101550507255-n870-ab-rows-12-probe{number}-candidate.log")), text).unwrap();
    }
    std::fs::write(logs.path().join("1794101633661375-precheck.log"), probe(40)).unwrap();
    let jobs = jobs_per_probe(logs.path());
    assert_eq!(jobs, 7, "the median of the probes that offloaded, not counting the refused ones or other logs");
    let free = 8;
    let ids: Vec<String> = (1..=13).map(|lane| format!("lane-{lane}")).collect();
    let suspects = Suspects::default();
    assert_eq!(suspects.plan(&ids, &[], free, RefusalRate::default(), false).len() * jobs, 56, "one probe per free slot, as the round planned it");
    let planned = suspects.plan(&ids, &[], fitting(free, jobs), RefusalRate::default(), false).len();
    assert!(planned >= 1 && planned * jobs <= free, "{planned} probes of {jobs} jobs on {free} slots");
}

// KEEP(bug): at 10:20 AM on 2026-11-08 the lander planned candidates of 19, 19, 19, 19, 1, 2 and 16 lanes in rounds that kept refusing a lane, and an urgent lander fix waited behind them
#[test]
fn after_two_refusals_in_the_last_hour_a_round_of_fourteen_builds_no_probe_over_two_lanes_and_the_urgent_ticket_rides_alone() {
    let queue = tempfile::tempdir().unwrap();
    for refused in [false, false, true, false, false, false, true, false] {
        Refusals::record(queue.path(), refused, 1).unwrap();
    }
    let ids: Vec<String> = (1..=14).map(|lane| format!("lane-{lane}")).collect();
    let planned = Suspects::default().plan(&ids, &[0], 6, Refusals::load(queue.path()).rate(), false);
    assert_eq!(planned.first(), Some(&vec![0]), "planned {planned:?}");
    assert!(planned.iter().all(|probe| probe.len() <= 2), "planned {planned:?}");
    let calm = tempfile::tempdir().unwrap();
    assert_eq!(Suspects::default().plan(&ids, &[], 6, Refusals::load(calm.path()).rate(), false).last().map(Vec::len), Some(14));
}
