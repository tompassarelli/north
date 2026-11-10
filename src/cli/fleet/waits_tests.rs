use super::*;

fn names(probes: Probes) -> Vec<&'static str> {
    probes.jobs().into_iter().map(|job| job.name).collect()
}

/// On 2026-11-11 nexus's tick spent 6 s of CPU in every 30 s tick on the scoreboard report, which a 78 s read never finished, on a 3-core host the laptop's tick already reports for (north#1036).
#[test]
fn a_host_that_opts_out_of_fleet_reports_runs_no_goals_scoreboard_or_corrections_probe() {
    let quiet = names(Probes::Fleet { reports: false });
    let full = names(Probes::Fleet { reports: true });
    for report in ["probes.settle.goals", "probes.settle.scoreboard", "probes.settle.corrections"] {
        assert!(full.contains(&report) && !quiet.contains(&report), "{report}");
    }
    assert!(quiet.contains(&"probes.records_mirror") && quiet.contains(&"probes.settle.reap"));
}
