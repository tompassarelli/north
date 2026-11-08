use super::Suspects;

#[test]
fn a_round_judged_on_this_host_checks_one_probe_however_many_slots_the_build_hosts_report() {
    let ids: Vec<String> = (1..=7).map(|lane| format!("lane-{lane}")).collect();
    let suspects = Suspects::default();
    assert_eq!(suspects.plan(&ids, 7, false).len(), 7);
    assert_eq!(suspects.plan(&ids, 7, true).len(), 1, "on 2026-11-08 at 7:06 AM the lander, falling back to this host, ran 8 probes of 7 lanes at once in place and drove the laptop to load 123 while main stood still for 18 minutes");
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
    assert_eq!(suspects.plan(&ids, free, false).len() * jobs, 56, "one probe per free slot, as the round planned it");
    let planned = suspects.plan(&ids, fitting(free, jobs), false).len();
    assert!(planned >= 1 && planned * jobs <= free, "{planned} probes of {jobs} jobs on {free} slots");
}
