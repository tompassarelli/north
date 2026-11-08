use super::Unbuilt;
use crate::land::checks::Verdict;
use crate::land::probes::{End, Probe};
use std::path::Path;

fn probe(dir: &Path, at: usize, lanes: &[usize], log: &str) -> Probe {
    let path = dir.join(format!("probe{at}.log"));
    std::fs::write(&path, log).unwrap();
    Probe { lanes: lanes.to_vec(), head: format!("{at:040}"), tips: Vec::new(), worktree: dir.to_path_buf(), log: path, end: End::Checked(Verdict::Refused("check failed".to_owned())) }
}

#[test]
fn a_lane_that_fails_to_link_in_two_probes_while_a_probe_without_it_builds_is_pulled_after_that_round() {
    let dir = tempfile::tempdir().unwrap();
    let queue = dir.path();
    let ids: Vec<String> = ["n722-slow1", "am-judge-renames", "n882-plan", "n842-recorder", "n706-guards3"].iter().map(|lane| format!("{lane}@head")).collect();
    let unlinked = "error: linking with `cc` failed: exit status: 1\n  = note: rust-lld: error: unable to find library -lseccomp\nerror: could not compile `north-land-sandbox` (lib) due to 1 previous error\nsafe-push: verdict refused\n";
    let built = "replayed 26534 cases in 1.53 s (debug build)\nrefused: admission call took 97.5 ms, over the 10 ms of replay_call_milliseconds\nsafe-push: verdict refused\n";
    let finished = [probe(queue, 0, &[1, 2, 3, 4], unlinked), probe(queue, 1, &[0, 2, 3, 4], unlinked), probe(queue, 2, &[0, 1, 2, 4], built)];
    let mut unbuilt = Unbuilt::load(queue, &ids);
    let pull = unbuilt.after(&ids, &finished).expect("on 2026-11-08 n842-recorder failed to link -lseccomp in both 8:07 AM probes it rode while the probe without it built, and every round stayed refused until a manual withdraw at 8:21 AM");
    assert_eq!((ids[pull.lane].as_str(), pull.probe, pull.failed), ("n842-recorder@head", 0, 2));
    assert_eq!(pull.error, "rust-lld: error: unable to find library -lseccomp");
    unbuilt.save(queue).unwrap();
    let mut reloaded = Unbuilt::load(queue, &ids);
    assert!(reloaded.after(&ids, &finished[..1]).is_none(), "a round with no probe that built pulls nothing");
}
