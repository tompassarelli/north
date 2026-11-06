use super::culprit;
use crate::land::checks::Verdict;

fn search(lanes: usize, room: usize, red_from: usize, base_red: bool) -> (Option<usize>, Vec<Vec<usize>>) {
    let mut rounds = Vec::new();
    let found = culprit(lanes, room, |counts| {
        rounds.push(counts.to_vec());
        counts.iter().map(|&count| if count >= red_from || base_red { Verdict::RedMain(format!("first {count}")) } else { Verdict::Green }).collect()
    });
    (found, rounds)
}

#[test]
fn four_merged_landings_with_the_third_red_name_it_after_one_parallel_round() {
    let (found, rounds) = search(4, 6, 3, false);
    assert_eq!(found, Some(2));
    assert_eq!(rounds, vec![vec![0, 1, 2, 3]], "the base and every proper prefix are checked in one round");
}

#[test]
fn too_few_free_slots_fall_back_to_bisection() {
    let (found, rounds) = search(4, 3, 3, false);
    assert_eq!(found, Some(2));
    assert!(rounds.iter().all(|round| round.len() == 1), "{rounds:?}");
}

#[test]
fn a_base_already_red_blames_no_lane() {
    assert_eq!(search(4, 6, 1, true).0, None);
    let (found, rounds) = search(4, 1, 1, true);
    assert_eq!(found, None);
    assert_eq!(rounds.last(), Some(&vec![0]), "bisection ending on the first lane probes the base");
}

#[test]
fn an_undecided_prefix_before_the_first_red_names_no_lane() {
    let found = culprit(3, 6, |counts| counts.iter().map(|&count| if count == 1 { Verdict::Transient("slot".into()) } else if count == 2 { Verdict::RedMain(String::new()) } else { Verdict::Green }).collect());
    assert_eq!(found, None);
}

#[test]
fn a_parallel_round_left_undecided_by_busy_slots_falls_back_to_bisection() {
    let mut rounds = 0;
    let found = culprit(4, 6, |counts| {
        rounds += 1;
        counts.iter().map(|&count| if counts.len() > 1 && count == 1 { Verdict::Transient("slots busy".into()) } else if count >= 3 { Verdict::RedMain(String::new()) } else { Verdict::Green }).collect()
    });
    assert_eq!(found, Some(2));
    assert!(rounds > 1);
}
