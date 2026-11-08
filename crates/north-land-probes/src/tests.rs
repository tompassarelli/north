use super::{plan, settle};

struct Lander {
    main: Vec<u32>,
    queue: Vec<u32>,
    suspects: Vec<Vec<u32>>,
    bad: u32,
}

impl Lander {
    fn round(&mut self, slots: usize) -> Vec<u32> {
        let probes = plan(&self.queue, slots, usize::MAX, &self.suspects);
        assert!(probes.len() <= slots, "round planned {} probes on {slots} slots", probes.len());
        let checked: Vec<Vec<u32>> = probes.iter().map(|probe| self.main.iter().chain(probe).copied().collect()).collect();
        let green: Vec<bool> = checked.iter().map(|tree| !tree.contains(&self.bad)).collect();
        let settled = settle(&probes, &green, &self.suspects);
        let landed: Vec<u32> = self.main.iter().chain(&settled.land).copied().collect();
        if !settled.land.is_empty() {
            assert!(checked.iter().zip(&green).any(|(tree, green)| *green && *tree == landed), "landed {landed:?}, a tree no probe of this round checked green");
        }
        self.queue.retain(|lane| !settled.land.contains(lane) && !settled.refuse.contains(lane));
        self.suspects = settled.groups;
        self.main = landed;
        settled.land
    }
}

#[test]
fn a_bad_seventh_lane_of_fourteen_on_six_slots_lands_lanes_one_to_six_in_round_one_and_the_other_seven_green_lanes_in_round_two() {
    let mut lander = Lander { main: Vec::new(), queue: (1..=14).collect(), suspects: Vec::new(), bad: 7 };
    assert_eq!(lander.round(6), (1..=6).collect::<Vec<_>>());
    assert_eq!(lander.round(6), (8..=14).collect::<Vec<_>>());
    assert_eq!(lander.main, (1..=6).chain(8..=14).collect::<Vec<_>>());
    assert!(lander.queue.is_empty(), "lane 7 failed alone on the clear lanes in round two and was not refused: queue {:?}", lander.queue);
}

#[test]
fn a_red_stride_cancels_the_running_probes_that_contain_it_and_no_other() {
    let queue: Vec<u32> = (1..=14).collect();
    let probes = plan(&queue, 6, usize::MAX, &[]);
    let red = probes.iter().position(|probe| probe.len() == 8).unwrap();
    let cancelled: Vec<usize> = super::cancelled(&probes, red, &[true; 6]);
    assert_eq!(cancelled.iter().map(|at| probes[*at].len()).collect::<Vec<_>>(), [11, 14]);
    assert!(super::cancelled(&probes, red, &[false; 6]).is_empty());
    let widest = probes.len() - 1;
    assert_eq!(super::superseded(&probes, widest, &[true; 6]).len(), widest);
    assert!(super::superseded(&probes, 0, &[true; 6]).is_empty());
}

fn replay_8_33_am(slots: usize) -> (usize, Vec<&'static str>, usize) {
    let suspects = ["n882-plan", "n882-tax", "n882-context", "am-move", "n842-join", "n706-guards3", "n706-cost-words", "n882-query"];
    let clear = ["n882-loops", "n876-t3-fixed-default", "n882-guards", "n842-recorder", "n870-ab-rows"];
    let bad = ["n882-plan", "n882-context"];
    let mut queue: Vec<&str> = suspects.iter().chain(&clear).copied().collect();
    let (mut named, mut landed, mut refused) = (vec![suspects.to_vec()], 0, Vec::new());
    for round in 1..=12 {
        let probes = plan(&queue, slots, usize::MAX, &named);
        assert!(!probes.is_empty() && probes.len() <= slots, "planned {probes:?} on {slots} slots");
        let green: Vec<bool> = probes.iter().map(|probe| !probe.iter().any(|lane| bad.contains(lane))).collect();
        let settled = settle(&probes, &green, &named);
        queue.retain(|lane| !settled.land.contains(lane) && !settled.refuse.contains(lane));
        landed += settled.land.len();
        refused.extend(settled.refuse);
        named = settled.groups;
        if refused.len() == bad.len() {
            refused.sort_unstable();
            return (round, refused, landed + queue.len());
        }
    }
    panic!("{slots} slots left {bad:?} unrefused after 12 rounds: refused {refused:?}, queue {queue:?}");
}

#[test]
fn the_eight_suspects_of_the_8_33_am_round_with_two_bad_lanes_are_both_refused_within_two_rounds_on_eight_slots_and_eight_on_one() {
    for (slots, rounds) in [(8, 2), (2, 5), (1, 8)] {
        let (took, refused, green) = replay_8_33_am(slots);
        assert_eq!(refused, ["n882-context", "n882-plan"]);
        assert!(took <= rounds, "at 8:33 AM every one of 8 leave-one-out probes carried a bad lane and all 8 were refused after 11 minutes; {slots} slots took {took} rounds to refuse both");
        assert_eq!(green, 11, "{slots} slots lost a green lane");
    }
}
