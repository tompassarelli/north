use std::collections::BTreeSet;

use super::{Judged, declared, judge};
use crate::land::replay::Replayed;
use north_landing::landing::red_check::Failure;

fn replayed(verdict: &str, failures: &[&str]) -> Replayed {
    Replayed { verdict: verdict.to_owned(), failures: failures.iter().map(|name| Failure::Cargo((*name).to_owned())).collect() }
}

#[test]
fn machinery_lane_replays_rounds_and_is_refused_for_a_verdict_it_flips_unless_its_patch_declares_it() {
    let round = "lane-a,lane-b";
    let none = BTreeSet::new();
    let landed = || replayed("landed", &[]);
    let refused = replayed("failed land check: gate-x", &[]);
    assert_eq!(judge(round, "landed abc", &landed(), || panic!("a matching replay asks main nothing"), &none), Judged::Matches);
    assert!(matches!(judge(round, "landed abc", &refused, landed, &none), Judged::Refused(why) if why.contains("Replay-flip: lane-a,lane-b :: WHY")), "a lane that turns a landed round refused, as main still lands it, is refused");
    let flip = declared("+++ b/docs/replay-flips.md\n+- Replay-flip: lane-a,lane-b :: an unverified landing is refused now\n+- Replay-flip: lane-c :: \n");
    assert_eq!(judge(round, "landed abc", &refused, landed, &flip), Judged::Declared, "a declared flip lands for the reviewers to accept");
    assert_eq!(judge(round, "failed gate-x", &landed(), || replayed("failed gate-x", &[]), &flip), Judged::Declared);
    let unreached = replayed("transient every build host exited 75", &[]);
    assert!(matches!(judge(round, "landed abc", &unreached, landed, &flip), Judged::Refused(_)), "a recorded verdict the lane turns into no verdict refuses it, declared or not");
    assert!(matches!(judge(round, "failed gate-x", &landed(), unreached_main, &none), Judged::Unreached(_)), "a flip main cannot confirm reaches no verdict, never a pass");
    assert_eq!(judge(round, "failed gate-x", &landed(), unreached_main, &flip), Judged::Declared);
    assert_eq!(flip.len(), 1, "a declaration with no reason declares nothing");
    assert!(matches!(judge(round, "landed abc", &unreached, unreached_main, &none), Judged::Unreached(_)));
    let case = replayed("failed nextest", &["worker_sandbox::a_disclosure_judge_runs_outside"]);
    assert!(matches!(judge(round, "landed abc", &case, || replayed("failed nextest", &[]), &flip), Judged::Refused(why) if why.contains("a_disclosure_judge_runs_outside")), "a case that passes under main's machinery and fails under the lane's refuses it");
    assert_eq!(judge(round, "landed abc", &case, || replayed("failed nextest", &["worker_sandbox::a_disclosure_judge_runs_outside"]), &none), Judged::Shared, "a failure main shares is not the lane's");
}

fn unreached_main() -> Replayed {
    replayed("error the lander's replay worktrees are all in use", &[])
}
