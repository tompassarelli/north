use std::collections::BTreeSet;

use super::{Probe, Suspects, culprit, culprits, suspects};
use crate::land::checks::Verdict;

fn searched_signatures(lanes: usize, suspects: &Suspects, bad: &[(usize, &str)], signatures: &[&str]) -> (Option<Vec<usize>>, usize) {
    let signatures: BTreeSet<String> = signatures.iter().map(|signature| (*signature).to_owned()).collect();
    let mut runs = 0;
    let found = culprits(lanes, suspects, &signatures, |counts| {
        runs += counts.len();
        counts
            .iter()
            .map(|&count| {
                let failing: BTreeSet<String> = bad.iter().filter(|(at, _)| *at < count).map(|(_, signature)| (*signature).to_owned()).collect();
                Probe { verdict: if failing.is_empty() { Verdict::Green } else { Verdict::RedMain(format!("first {count}")) }, failing }
            })
            .collect()
    });
    (found, runs)
}

fn search(lanes: usize, room: usize, red_from: usize, base_red: bool) -> (Option<usize>, Vec<Vec<usize>>) {
    searched(lanes, room, &Suspects::Batch, red_from, base_red)
}

fn searched(lanes: usize, room: usize, suspects: &Suspects, red_from: usize, base_red: bool) -> (Option<usize>, Vec<Vec<usize>>) {
    let mut rounds = Vec::new();
    let found = culprit(lanes, room, suspects, |counts| {
        rounds.push(counts.to_vec());
        counts.iter().map(|&count| if count >= red_from || base_red { Verdict::RedMain(format!("first {count}")) } else { Verdict::Green }).collect()
    });
    (found, rounds)
}

fn batch() -> Vec<String> {
    ["lane-a", "lane-b", "lane-c", "lane-d"].map(str::to_owned).to_vec()
}

#[test]
fn four_merged_landings_with_the_third_red_name_it_after_one_parallel_round() {
    let (found, rounds) = search(4, 6, 3, false);
    assert_eq!(found, Some(2));
    assert_eq!(rounds, vec![vec![0, 1, 2, 3]], "the base and every proper prefix are checked in one round");
}

#[test]
fn a_base_already_red_blames_no_lane() {
    assert_eq!(search(4, 6, 1, true).0, None);
    let (found, rounds) = search(4, 1, 1, true);
    assert_eq!(found, None);
    assert_eq!(rounds.last(), Some(&vec![0]), "bisection ending on the first lane probes the base");
}

#[test]
fn a_red_four_lane_batch_whose_graph_blame_names_one_lane_reverts_it_with_no_bisect_round() {
    let named = suspects(&batch(), Some(0), "lane-c\n");
    assert_eq!(named, Suspects::Named(vec![2]));
    let (found, rounds) = searched(4, 1, &named, 3, false);
    assert_eq!(found, Some(2));
    assert!(rounds.is_empty(), "no prefix is checked: {rounds:?}");
}

#[test]
fn a_red_four_lane_batch_whose_graph_blame_names_two_lanes_bisects_only_those_two() {
    let named = suspects(&batch(), Some(0), "lane-c\nlane-a\n");
    assert_eq!(named, Suspects::Named(vec![0, 2]));
    let (found, rounds) = searched(4, 1, &named, 3, false);
    assert_eq!(found, Some(2));
    assert_eq!(rounds, vec![vec![1]], "only the prefix through lane-a, the first named lane, is checked");
    let (found, rounds) = searched(4, 1, &named, 1, false);
    assert_eq!(found, Some(0));
    assert_eq!(rounds, vec![vec![1]]);
    let eight: Vec<String> = (0..8).map(|at| format!("lane-{at}")).collect();
    let (found, runs) = searched_signatures(8, &Suspects::Batch, &[(0, "step citations"), (7, "step layers")], &["step citations", "step layers"]);
    assert_eq!(found, Some(vec![0, 7]));
    assert!(runs <= 6, "two bad lanes among eight must cost at most 2·log2(8) bisect runs, not {runs}");
    let named = suspects(&eight, Some(0), "lane-0\nlane-7\n");
    assert_eq!(named, Suspects::Named(vec![0, 7]));
    let (found, runs) = searched_signatures(8, &named, &[(0, "step citations"), (7, "step layers")], &["step citations", "step layers"]);
    assert_eq!((found, runs), (Some(vec![0, 7]), 1), "the red run already checked both named lanes' full prefix");
}

#[test]
fn a_red_four_lane_batch_whose_graph_blame_is_untraced_or_missing_bisects_all_four_as_before() {
    for (exit, printed) in [(Some(3), ""), (Some(2), ""), (Some(0), "lane-z\n"), (None, "lane-c\n")] {
        assert_eq!(suspects(&batch(), exit, printed), Suspects::Batch, "exit {exit:?} printing {printed:?}");
    }
    let (found, rounds) = searched(4, 1, &suspects(&batch(), Some(3), ""), 3, false);
    assert_eq!(found, Some(2));
    assert_eq!(rounds, vec![vec![2], vec![3]], "the whole batch is bisected by its prefixes");
}
