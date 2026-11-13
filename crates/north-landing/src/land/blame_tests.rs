use std::collections::{BTreeMap, BTreeSet};

use super::{Blame, Probe, Suspects, culprit, culprits, read, suspects};
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

fn search(lanes: usize, room: usize, red_from: usize, base_red: bool) -> (Blame, Vec<Vec<usize>>) {
    searched(lanes, room, &Suspects::Untraced, red_from, base_red)
}

fn searched(lanes: usize, room: usize, suspects: &Suspects, red_from: usize, base_red: bool) -> (Blame, Vec<Vec<usize>>) {
    let mut rounds = Vec::new();
    let found = culprit(lanes, room, suspects, |counts| {
        rounds.push(counts.to_vec());
        counts.iter().map(|&count| if count >= red_from || base_red { Verdict::RedMain(format!("first {count}")) } else { Verdict::Green }).collect()
    });
    (found, rounds)
}

fn printed(names: &[&str]) -> String {
    let edges: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::from([("north-ops lane_gc", names.iter().map(|name| (*name, "reaches")).collect())]);
    serde_json::json!({ "lanes": names, "known": { "north-ops lane_gc": true }, "edges": edges, "untraced": [], "map_age_secs": 60 }).to_string()
}

fn trusted(known: bool, age: u64) -> String {
    serde_json::json!({ "lanes": [], "known": { "north-ops lane_gc": known }, "edges": {}, "untraced": [], "map_age_secs": age }).to_string()
}

fn batch() -> Vec<String> {
    ["lane-a", "lane-b", "lane-c", "lane-d"].map(str::to_owned).to_vec()
}

#[test]
fn four_merged_landings_with_the_third_red_name_it_after_one_parallel_round() {
    let (found, rounds) = search(4, 6, 3, false);
    assert_eq!(found, Blame::Lane(2));
    assert_eq!(rounds, vec![vec![0, 1, 2, 3]], "the base and every proper prefix are checked in one round");
}

#[test]
fn a_base_already_red_blames_no_lane() {
    assert_eq!(search(4, 6, 1, true).0, Blame::BaseRed);
    let (found, rounds) = search(4, 1, 1, true);
    assert_eq!(found, Blame::BaseRed);
    assert_eq!(rounds.last(), Some(&vec![0]), "bisection ending on the first lane probes the base");
}

#[test]
fn a_red_four_lane_batch_whose_graph_blame_names_one_lane_blames_it_only_once_its_parent_passes() {
    let named = suspects(&batch(), Some(0), &printed(&["lane-c"]));
    assert_eq!(named, Suspects::Named(vec![2]));
    let (found, rounds) = searched(4, 1, &named, 3, false);
    assert_eq!(found, Blame::Lane(2));
    assert_eq!(rounds, vec![vec![2, 3]], "only the named lane's parent and its own prefix are checked");
    let (found, rounds) = searched(4, 1, &named, 1, false);
    assert_eq!(found, Blame::Lane(0), "a named lane whose parent is already red is not blamed");
    assert_eq!(rounds, vec![vec![2, 3], vec![1], vec![0]]);
}

#[test]
fn a_red_four_lane_batch_whose_graph_blame_names_two_lanes_bisects_only_those_two() {
    let named = suspects(&batch(), Some(0), &printed(&["lane-c", "lane-a"]));
    assert_eq!(named, Suspects::Named(vec![0, 2]));
    let (found, rounds) = searched(4, 1, &named, 3, false);
    assert_eq!(found, Blame::Lane(2));
    assert_eq!(rounds, vec![vec![1], vec![2, 3]], "only the prefix through lane-a, the first named lane, is checked, then lane-c's parent and own prefix");
    let (found, rounds) = searched(4, 1, &named, 1, false);
    assert_eq!(found, Blame::Lane(0));
    assert_eq!(rounds, vec![vec![1], vec![0]]);
}

#[test]
fn two_bad_lanes_among_eight_are_isolated_in_at_most_six_bisect_runs() {
    let eight: Vec<String> = (0..8).map(|at| format!("lane-{at}")).collect();
    let (found, runs) = searched_signatures(8, &Suspects::Untraced, &[(0, "step citations"), (7, "step layers")], &["step citations", "step layers"]);
    assert_eq!(found, Some(vec![0, 7]));
    assert!(runs <= 6, "two bad lanes among eight must cost at most 2·log2(8) bisect runs, not {runs}");
    let named = suspects(&eight, Some(0), &printed(&["lane-0", "lane-7"]));
    assert_eq!(named, Suspects::Named(vec![0, 7]));
    let (found, runs) = searched_signatures(8, &named, &[(0, "step citations"), (7, "step layers")], &["step citations", "step layers"]);
    assert_eq!((found, runs), (Some(vec![0, 7]), 1), "the red run already checked both named lanes' full prefix");
}

#[test]
fn a_red_four_lane_batch_whose_graph_blame_is_untraced_or_missing_bisects_all_four_as_before() {
    for (exit, printed) in [(Some(3), ""), (Some(2), ""), (Some(0), "lane-z\n"), (None, &printed(&["lane-c"]))] {
        assert_eq!(suspects(&batch(), exit, printed), Suspects::Untraced, "exit {exit:?} printing {printed:?}");
    }
    let (found, rounds) = searched(4, 1, &suspects(&batch(), Some(3), ""), 3, false);
    assert_eq!(found, Blame::Lane(2));
    assert_eq!(rounds, vec![vec![2], vec![3]], "the whole batch is bisected by its prefixes");
}

#[test]
fn a_one_lane_batch_whose_lane_the_graph_names_is_one_suspect() {
    let one = ["lane-a".to_owned()];
    let (named, why) = read(&one, Some(0), &printed(&["lane-a"]));
    assert_eq!(named, Suspects::Named(vec![0]));
    assert!(why.contains("lane-a through the reaches edge for north-ops lane_gc"), "{why}");
    let (none, why) = read(&one, Some(3), &trusted(true, 60));
    assert_eq!(none, Suspects::NoSuspect);
    assert!(why.contains("no lane reaches the known north-ops lane_gc"), "{why}");
    assert_eq!(suspects(&one, Some(3), &trusted(true, 4 * 86_400)), Suspects::Untraced);
}
