use super::{Suspects, culprit, suspects};
use crate::land::checks::Verdict;

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
