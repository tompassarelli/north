use super::{refused, since_instant};
use crate::foundation::clock::Timestamp;
use crate::platform::records_mirror::{ROOT, Records, STAMP, Stamp};
use std::fs;

fn mirrored(state: &std::path::Path, at: f64) {
    let root = state.join(ROOT);
    let stamp = Stamp { host: "whiterabbit".into(), state: "/home/tom/.local/state".into(), at };
    fs::create_dir_all(root.join(STAMP).parent().unwrap()).unwrap();
    fs::write(root.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
}

#[test]
fn a_stale_lander_mirror_is_refused_not_passed() {
    let state = tempfile::tempdir().unwrap();
    let now = 1_794_270_600.0;
    mirrored(state.path(), now - 26.0 * 3600.0);
    let records = Records::of(state.path(), "gex45");
    let why = refused(&records, "2026-11-09T00:30:00Z", None, now).expect("a 26 h old mirror cannot judge this window");
    assert!(why.contains("stale mirror"), "{why}");
    assert!(why.contains("whiterabbit"), "{why}");
    assert!(why.contains("run it there"), "{why}");

    let fresh = tempfile::tempdir().unwrap();
    mirrored(fresh.path(), now - 30.0);
    assert_eq!(refused(&Records::of(fresh.path(), "gex45"), "2026-11-09T00:30:00Z", None, now), None, "a mirror the fleet tick just refreshed still answers");

    let local = tempfile::tempdir().unwrap();
    mirrored(local.path(), now - 26.0 * 3600.0);
    assert_eq!(refused(&Records::of(local.path(), "whiterabbit"), "2026-11-09T00:30:00Z", None, now), None, "the lander host reads its own records, however old they are");
}

#[test]
fn a_full_run_failure_names_the_lane_whose_own_run_first_failed_the_test_not_a_later_lane_landed_on_red_main() {
    use super::responsible::{Blame, responsible};
    use std::collections::{BTreeMap, BTreeSet};
    let test = "worker_sandbox::a_command_confined_to_its_lane_reads_its_lane_and_not_another_project_the_sops_keys_or_agent_state";
    let chain: Vec<String> = ["5f42fb5463", "95afbfbcab", "ce30b6ea43"].map(str::to_owned).to_vec();
    let red = |sha: &str| (sha.to_owned(), BTreeSet::from([test.to_owned()]));
    let runs = BTreeMap::from([red("5f42fb5463"), red("ce30b6ea43")]);
    let mut landed = BTreeMap::from([("5f42fb5463".to_owned(), vec!["ds-1033".to_owned()]), ("95afbfbcab".to_owned(), vec!["dash".to_owned()]), ("ce30b6ea43".to_owned(), vec!["later".to_owned()])]);
    assert_eq!(responsible(&chain, &runs, &landed, test), Blame::Lane { lane: "ds-1033".into(), sha: "5f42fb5463".into() });
    landed.insert("5f42fb5463".to_owned(), vec!["ds-1033".to_owned(), "batched".to_owned()]);
    assert!(matches!(responsible(&chain, &runs, &landed, test), Blame::Unjudged(why) if why.contains("bisection")), "a batch gives no single lane without the lander's bisection");
}

#[test]
fn a_relative_since_becomes_a_date_gh_can_filter_on() {
    let now = Timestamp::parse_rfc3339("2026-11-10T15:00:00Z").unwrap();
    assert_eq!(since_instant("24h", now), "2026-11-09T15:00:00Z");
    assert_eq!(since_instant("2026-11-09T01:00:00Z", now), "2026-11-09T01:00:00Z");
}
