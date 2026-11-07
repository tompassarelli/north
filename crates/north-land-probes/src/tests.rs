use super::{plan, settle};

struct Lander {
    main: Vec<u32>,
    queue: Vec<u32>,
    suspects: Vec<u32>,
    bad: u32,
}

impl Lander {
    fn round(&mut self, slots: usize) -> Vec<u32> {
        let probes = plan(&self.queue, slots, usize::MAX, &self.suspects);
        assert!(probes.len() <= slots, "round planned {} probes on {slots} slots", probes.len());
        let checked: Vec<Vec<u32>> = probes.iter().map(|probe| self.main.iter().chain(probe).copied().collect()).collect();
        let green: Vec<bool> = checked.iter().map(|tree| !tree.contains(&self.bad)).collect();
        let settled = settle(&probes, &green);
        let landed: Vec<u32> = self.main.iter().chain(&settled.land).copied().collect();
        if !settled.land.is_empty() {
            assert!(checked.iter().zip(&green).any(|(tree, green)| *green && *tree == landed), "landed {landed:?}, a tree no probe of this round checked green");
        }
        self.queue.retain(|lane| !settled.land.contains(lane) && settled.refuse != Some(*lane));
        self.suspects = settled.suspects;
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
    assert_eq!(lander.queue, [7]);
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
