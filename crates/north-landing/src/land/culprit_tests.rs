use std::cell::RefCell;

use super::*;

struct Clash {
    pair: (usize, usize),
    room: usize,
    moved: bool,
    transient: Option<usize>,
    waves: RefCell<Vec<Vec<Vec<usize>>>>,
}

impl Clash {
    fn new(pair: (usize, usize), room: usize) -> Self {
        Self { pair, room, moved: false, transient: None, waves: RefCell::new(Vec::new()) }
    }
}

fn number(entry: &Entry) -> usize {
    entry.1.branch.trim_start_matches("lane-").parse().unwrap_or(0)
}

impl Bench for Clash {
    fn room(&self, wanted: usize) -> usize {
        self.room.min(wanted)
    }

    fn check(&self, sets: &[&[Entry]]) -> Result<Vec<Checked>, String> {
        let lanes: Vec<Vec<usize>> = sets.iter().map(|set| set.iter().map(number).collect()).collect();
        self.waves.borrow_mut().push(lanes.clone());
        Ok(lanes
            .into_iter()
            .map(|set| {
                let name = set.iter().map(usize::to_string).collect::<Vec<_>>().join("-");
                let verdict = if set.contains(&self.pair.0) && set.contains(&self.pair.1) {
                    Verdict::Refused("clash".to_owned())
                } else if self.transient == Some(set.len()) && set.len() > 1 {
                    Verdict::Transient("fetch".to_owned())
                } else {
                    Verdict::Green
                };
                Checked { verdict, head: format!("head-{name}"), log: PathBuf::from(format!("{name}.log")) }
            })
            .collect())
    }

    fn moved(&self) -> Result<bool, String> {
        Ok(self.moved)
    }
}

fn lanes(count: usize) -> Vec<(PathBuf, Ticket)> {
    (1..=count)
        .map(|lane| {
            let ticket = Ticket {
                origin: "origin".to_owned(),
                root: PathBuf::from("/repo"),
                host: "host".to_owned(),
                branch: format!("lane-{lane}"),
                head: format!("tip-{lane}"),
                session: None,
                queued_at: String::new(),
                first_commit_secs: None,
                several_commits: None,
                red_fix: None,
                verdict: None,
                keep_lane: false,
            };
            (PathBuf::from(format!("/queue/{lane}.json")), ticket)
        })
        .collect()
}

fn together(count: usize) -> Checked {
    Checked { verdict: Verdict::Refused("candidate".to_owned()), head: "head-all".to_owned(), log: PathBuf::from(format!("{count}-lanes.log")) }
}

type Landed = (usize, String, usize, String, PathBuf);

fn run(bench: &Clash, count: usize) -> (Option<Landed>, Option<String>) {
    let queued = lanes(count);
    let included: Vec<Entry> = queued.iter().map(|(path, ticket)| (path, ticket)).collect();
    let mut strikes = Strikes::default();
    match find(bench, &included, together(count), &mut strikes) {
        Ok(Outcome::Interaction { lanes, head, partner: (entry, checked, detail) }) => (Some((lanes, head, number(&entry), detail, checked.log)), None),
        Ok(Outcome::Held(reason)) => (None, Some(reason)),
        Ok(Outcome::Moved) => (None, Some("moved".to_owned())),
        Ok(Outcome::Paused(detail)) => (None, Some(format!("paused {detail}"))),
        Ok(Outcome::Culprits(_)) => (None, Some("culprits".to_owned())),
        Err(error) => (None, Some(error)),
    }
}

#[test]
fn an_interaction_lands_the_longest_green_prefix_from_one_parallel_wave() {
    let bench = Clash::new((3, 5), MOST_AT_ONCE);
    let (found, other) = run(&bench, 6);
    assert_eq!(other, None);
    assert_eq!(found, Some((4, "head-1-2-3-4".to_owned(), 5, "clash".to_owned(), PathBuf::from("1-2-3-4-5.log"))));
    let waves = bench.waves.into_inner();
    assert_eq!(waves, vec![(1..=6).map(|lane| vec![lane]).collect::<Vec<_>>(), vec![vec![1, 2, 3, 4, 5], vec![1, 2, 3, 4], vec![1, 2, 3], vec![1, 2]]]);
}

#[test]
fn a_clashing_pair_lands_the_first_lane_without_another_check() {
    let bench = Clash::new((1, 2), MOST_AT_ONCE);
    let (found, other) = run(&bench, 2);
    assert_eq!(other, None);
    assert_eq!(found, Some((1, "head-1".to_owned(), 2, "candidate".to_owned(), PathBuf::from("2-lanes.log"))));
    assert_eq!(bench.waves.into_inner(), vec![vec![vec![1], vec![2]]]);
}

#[test]
fn short_slots_check_the_widest_prefixes_first() {
    let bench = Clash::new((1, 2), 2);
    let (found, other) = run(&bench, 6);
    assert_eq!(other, None);
    assert_eq!(found, Some((1, "head-1".to_owned(), 2, "clash".to_owned(), PathBuf::from("1-2.log"))));
    let waves = bench.waves.into_inner();
    assert_eq!(waves[3..], [vec![vec![1, 2, 3, 4, 5], vec![1, 2, 3, 4]], vec![vec![1, 2, 3], vec![1, 2]]]);
}

#[test]
fn a_transient_prefix_holds_the_lane_after_the_longest_green_one() {
    let bench = Clash { transient: Some(4), ..Clash::new((3, 5), MOST_AT_ONCE) };
    let (found, other) = run(&bench, 6);
    assert_eq!(found, None);
    assert_eq!(other, Some("lane-4 after the 3 lanes before it: transient fetch".to_owned()));
}

#[test]
fn a_moved_main_judges_no_lane() {
    let bench = Clash { moved: true, ..Clash::new((3, 5), MOST_AT_ONCE) };
    assert_eq!(run(&bench, 6).1, Some("moved".to_owned()));
    let pair = Clash { moved: true, ..Clash::new((1, 2), MOST_AT_ONCE) };
    assert_eq!(run(&pair, 2).1, Some("moved".to_owned()));
}

#[test]
fn a_repository_owns_its_slot_locks_and_no_other() {
    assert!(belongs("code-north-main.lock", "code-north-main"));
    assert!(belongs("code-north-main-3.lock", "code-north-main"));
    assert!(!belongs("code-north-main-3.holder", "code-north-main"));
    assert!(!belongs("code-north-main-x.lock", "code-north-main"));
    assert!(!belongs("code-north-mainly.lock", "code-north-main"));
    assert!(!belongs("code-makgora-main.lock", "code-north-main"));
}
