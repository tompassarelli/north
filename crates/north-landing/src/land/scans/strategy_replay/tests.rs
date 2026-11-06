use std::collections::BTreeSet;

use super::{beyond, refused};

const MAIN: &str = "landing-p90: 261 of 294 checked pass, BREACHED\nrefused: landing-p90: p90 is 65.9 s over 60 s; slowest landed north-621-p12a took 219 s\n";

#[test]
fn a_lane_is_refused_only_for_what_its_build_breaks_beyond_main_s_own_replay() {
    let main = refused(MAIN);
    let same = refused("refused: landing-p90: p90 is 66.2 s over 60 s; slowest landed north-621-p12a took 219 s\n");
    assert_eq!(beyond(&same, &main), Vec::<String>::new());
    let worse = refused("refused: landing-p90: p90 is 70.0 s over 60 s\nrefused: placement account: logged 2766, replayed 9000, past the x2 bound of replay_outcome_shift_ratio\nrefused: the replay took 9.1 s, over the 5 s of replay_budget_seconds\n");
    assert_eq!(beyond(&worse, &main), ["placement account", "the replay took"]);
    assert_eq!(refused("north strategy replay: 2026-11-05T09:20:55Z to 2026-11-06T09:20:55Z\n"), BTreeSet::new());
}
