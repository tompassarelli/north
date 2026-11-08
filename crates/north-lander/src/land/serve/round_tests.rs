use super::Suspects;

#[test]
fn a_round_judged_on_this_host_checks_one_probe_however_many_slots_the_build_hosts_report() {
    let ids: Vec<String> = (1..=7).map(|lane| format!("lane-{lane}")).collect();
    let suspects = Suspects::default();
    assert_eq!(suspects.plan(&ids, 7, false).len(), 7);
    assert_eq!(suspects.plan(&ids, 7, true).len(), 1, "on 2026-11-08 at 7:06 AM the lander, falling back to this host, ran 8 probes of 7 lanes at once in place and drove the laptop to load 123 while main stood still for 18 minutes");
}
