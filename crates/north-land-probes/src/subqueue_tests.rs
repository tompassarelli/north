use std::collections::BTreeSet;

use super::{Footprint, round};
use crate::settle;

struct Lander {
    main: Vec<u32>,
    queue: Vec<(u32, Footprint<&'static str>)>,
    suspects: Vec<u32>,
    green: fn(&[u32]) -> bool,
}

impl Lander {
    fn new(queue: &[(u32, &[&'static str])], green: fn(&[u32]) -> bool) -> Self {
        let queue = queue.iter().map(|(lane, crates)| (*lane, Footprint::Crates(crates.iter().copied().collect::<BTreeSet<_>>()))).collect();
        Self { main: Vec::new(), queue, suspects: Vec::new(), green }
    }

    fn round(&mut self, slots: usize) -> Vec<Vec<u32>> {
        let subrounds = round(&self.queue, slots, usize::MAX, &self.suspects);
        assert!(subrounds.iter().map(|subround| subround.probes.len()).sum::<usize>() <= slots, "round planned more probes than its {slots} slots");
        let base = self.main.clone();
        let mut landed = Vec::new();
        self.suspects.clear();
        for subround in subrounds {
            let green: Vec<bool> = subround.probes.iter().map(|probe| (self.green)(&base.iter().chain(probe).copied().collect::<Vec<_>>())).collect();
            let settled = settle(&subround.probes, &green);
            self.queue.retain(|(lane, _)| !settled.land.contains(lane) && settled.refuse != Some(*lane));
            self.suspects.extend(settled.suspects);
            self.main.extend(&settled.land);
            landed.push(settled.land);
        }
        landed
    }
}

#[test]
fn a_bad_first_lane_holds_neither_of_two_lanes_whose_footprints_are_disjoint_from_it_and_both_land_in_round_one() {
    let mut lander = Lander::new(&[(1, &["north-fleet"]), (2, &["north-capacity"]), (3, &["north-graph"])], |tree| !tree.contains(&1));
    assert_eq!(lander.round(3), [vec![], vec![2], vec![3]]);
    assert_eq!(lander.main, [2, 3]);
    assert_eq!(lander.queue.iter().map(|(lane, _)| *lane).collect::<Vec<_>>(), Vec::<u32>::new());
}

#[test]
fn two_lanes_that_pass_alone_but_break_when_merged_are_never_both_landed_on_any_number_of_slots() {
    for footprint in [&["north-landing"][..], &["north-landing", "north-base"][..]] {
        for slots in 1..=4 {
            let mut lander = Lander::new(&[(1, &["north-landing"]), (2, footprint)], |tree| !(tree.contains(&1) && tree.contains(&2)));
            for _ in 0..4 {
                lander.round(slots);
            }
            assert!(!(lander.main.contains(&1) && lander.main.contains(&2)), "{slots} slots landed both interacting lanes: main {:?}", lander.main);
            assert_eq!(lander.main.len() + lander.queue.len(), 1, "{slots} slots left main {:?} and queue of {}", lander.main, lander.queue.len());
        }
    }
}

#[test]
fn with_a_bad_first_lane_and_three_slots_the_second_lane_lands_in_round_one_through_its_bypass_candidate() {
    let mut lander = Lander::new(&[(1, &["north-landing"]), (2, &["north-landing"])], |tree| !tree.contains(&1));
    assert_eq!(round(&lander.queue, 3, usize::MAX, &[])[0].probes, [vec![1], vec![1, 2], vec![2]]);
    assert_eq!(lander.round(3), [vec![2]]);
    assert!(lander.queue.is_empty(), "lane 1 was not refused in round one");
}
