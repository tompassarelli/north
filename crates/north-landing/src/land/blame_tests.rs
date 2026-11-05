use super::*;

fn broken_from(lane: usize) -> impl FnMut(usize) -> Verdict {
    move |landed| if landed >= lane { Verdict::RedMain(format!("{landed} lanes")) } else { Verdict::Green }
}

#[test]
fn the_first_lane_whose_prefix_is_red_is_the_culprit() {
    for lanes in 1..=9 {
        for broken in 1..=lanes {
            assert_eq!(culprit(lanes, broken_from(broken)), Some(broken - 1), "{lanes} lanes, broken from {broken}");
        }
    }
}

#[test]
fn a_batch_of_n_lanes_takes_at_most_log_n_runs() {
    let mut runs = 0;
    culprit(8, |landed| {
        runs += 1;
        broken_from(6)(landed)
    });
    assert_eq!(runs, 3);
}

#[test]
fn a_single_lane_is_blamed_without_a_run() {
    assert_eq!(culprit(1, |_| panic!("probed")), Some(0));
}

#[test]
fn an_inconclusive_run_names_nobody() {
    assert_eq!(culprit(4, |_| Verdict::Transient("host lost".into())), None);
}
