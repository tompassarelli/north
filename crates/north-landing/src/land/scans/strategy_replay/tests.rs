use std::collections::BTreeSet;

use super::super::strategy_purity::tests::{assert_a_dependency_outside_the_allowlist_or_a_build_script_is_refused, assert_a_strategy_that_could_reach_the_host_is_refused_and_a_sealed_one_passes};
use super::{beyond, declared, figure_slower, refused, slower_than_main};

const MAIN: &str = "landing-p90: 261 of 294 checked pass, BREACHED\nrefused: landing-p90: p90 is 65.9 s over 60 s; slowest landed north-621-p12a took 219 s\n";

#[test]
fn a_lane_is_refused_only_for_what_its_build_breaks_beyond_main_s_own_replay() {
    let main = refused(MAIN);
    let same = refused("refused: landing-p90: p90 is 66.2 s over 60 s; slowest landed north-621-p12a took 219 s\n");
    assert_eq!(beyond(&same, &main), Vec::<String>::new());
    let worse = refused("refused: landing-p90: p90 is 70.0 s over 60 s\nrefused: placement account: logged 2766, replayed 9000, past the x2 bound of replay_outcome_shift_ratio\nrefused: the replay took 9.1 s, over the 5 s of replay_budget_seconds\n");
    assert_eq!(beyond(&worse, &main), ["placement account", "the replay took"]);
    let intended = declared("Keep boxes off cloud\n\nReplay-shift: placement account: the cloud lane closes while the credit is unusable\nReplay-shift: the replay took:\n");
    assert_eq!(beyond(&worse, &main).into_iter().filter(|key| !intended.contains_key(key)).collect::<Vec<_>>(), ["the replay took"], "an intended placement change had no way to land past the shift bound (north#690 on 2026-11-06); a declaration without a reason passes nothing");
    assert_eq!(refused("north strategy replay: 2026-11-05T09:20:55Z to 2026-11-06T09:20:55Z\n"), BTreeSet::new());
    assert!(slower_than_main(&report("9.80", "debug"), &report("2.10", "debug")), "a planted slow lane is named");
    assert!(!slower_than_main(&report("8.35", "debug"), &report("2.25", "release")), "i643-decision-diff was refused on 2026-11-06 at 8.35 s by comparing a debug build to main's release one");
    assert!(!slower_than_main(&report("8.35", "debug"), &report("7.90", "debug")), "an unchanged lane under the same host load is not named");
    assert!(!slower_than_main("no timing line\n", &report("2.10", "debug")));
    let measured = |profile: &str, call: &str, growth: &str| format!("measured: admission {call} ms, placement 0.4 ms, growth {growth} MiB\nreplayed 25828 cases in 2.63 s ({profile} build)\n");
    assert!(figure_slower(&measured("debug", "30.0", "10"), &measured("debug", "12.0", "10"), "admission"), "a planted slow strategy call is named");
    assert!(!figure_slower(&measured("debug", "16.7", "238"), &measured("release", "1.2", "230"), "admission"), "the 2026-11-06 debug lane was refused for calls and memory against main's release build");
    assert!(!figure_slower(&measured("debug", "16.7", "238"), &measured("debug", "15.0", "230"), "growth"), "an unchanged lane under the same load is not named");
    assert_a_strategy_that_could_reach_the_host_is_refused_and_a_sealed_one_passes();
    assert_a_dependency_outside_the_allowlist_or_a_build_script_is_refused();
}

fn report(seconds: &str, profile: &str) -> String {
    format!("placement account: logged 10, replayed 10\nreplayed 22745 cases in {seconds} s ({profile} build)\n")
}
