use stateright::{Checker, Model, Property};
use std::collections::BTreeSet;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Good,
    Bad,
    Latent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Verdict {
    Landed,
    Conflict,
    Culprit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    Plan,
    SpeculativePushUnchecked,
    SheriffRevertsWholeRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Pusher {
    Lander,
    Peer,
    Sheriff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Commit {
    by: Pusher,
    parent: u8,
    checked_base: u8,
    revert: bool,
    speculative: bool,
    lanes: u8,
    checked: Option<(u8, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Cand {
    base: u8,
    base_tree: u8,
    lanes: u8,
    speculative: bool,
    verdict: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Peer {
    Idle,
    Checked { base: u8, tree: u8 },
    Done(Verdict),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct State {
    kinds: Vec<Kind>,
    pending: u8,
    queue: u8,
    slots: Vec<Cand>,
    verdicts: Vec<Option<Verdict>>,
    peer: Peer,
    main: Vec<Commit>,
    greens: BTreeSet<(u8, u8)>,
    ci: Option<(u8, bool)>,
    last_green: u8,
    cas_lost: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Action {
    Enqueue(u8),
    Build,
    Speculate,
    Check(usize),
    Push,
    PushSpeculative,
    Bisect,
    PeerFetchCheck,
    PeerPush,
    RunCi,
    Sheriff,
}

struct Lander {
    lanes: u8,
    depth: usize,
    conflict: bool,
    variant: Variant,
    only: Option<&'static str>,
}

const EXACTNESS: &str = "exactness";
const NO_LOST_LANE: &str = "no lost lane";
const NO_DOUBLE_LANDING: &str = "no double landing";
const CULPRIT_ONLY: &str = "culprit-only ejection";
const CAS_SAFETY: &str = "cas safety";
const PROPERTIES: [&str; 5] = [EXACTNESS, NO_LOST_LANE, NO_DOUBLE_LANDING, CULPRIT_ONLY, CAS_SAFETY];

const REACH_SPECULATIVE_LANDED: &str = "reachable: a speculative candidate lands";
const REACH_CAS_LOST: &str = "reachable: the peer host moves main under a candidate";
const REACH_REVERT: &str = "reachable: the sheriff reverts a culprit";
const REACH_CULPRIT: &str = "reachable: bisection ejects a culprit from a multi-lane candidate";
const REACH_CONFLICT: &str = "reachable: a lane meets a conflict";
const REACHABLE: [&str; 5] = [REACH_SPECULATIVE_LANDED, REACH_CAS_LOST, REACH_REVERT, REACH_CULPRIT, REACH_CONFLICT];

impl Lander {
    fn plan(lanes: u8) -> Self {
        Lander { lanes, depth: 2, conflict: true, variant: Variant::Plan, only: None }
    }

    fn peer_bit(&self) -> u8 {
        1 << self.lanes
    }

    fn tree(&self, main: &[Commit], upto: u8) -> u8 {
        main[..upto as usize].iter().fold(0, |t, c| if c.revert { t & !c.lanes } else { t | c.lanes })
    }

    fn tip(&self, s: &State) -> u8 {
        s.main.len() as u8
    }

    fn lanes_of(&self, mask: u8) -> impl Iterator<Item = usize> + '_ {
        (0..self.lanes as usize).filter(move |l| mask & (1 << l) != 0)
    }

    fn land_green(&self, s: &State, tree: u8) -> bool {
        self.lanes_of(tree).all(|l| s.kinds[l] != Kind::Bad)
    }

    fn full_green(&self, s: &State, tree: u8) -> bool {
        self.lanes_of(tree).all(|l| s.kinds[l] == Kind::Good)
    }

    fn conflicting(&self, tree: u8, lanes: u8) -> u8 {
        if self.conflict && tree & self.peer_bit() != 0 { lanes & 1 } else { 0 }
    }

    fn admit(&self, s: &mut State, tree: u8, lanes: u8) -> u8 {
        let clash = self.conflicting(tree, lanes);
        for l in self.lanes_of(clash) {
            s.verdicts[l] = Some(Verdict::Conflict);
        }
        lanes & !clash
    }

    fn push(&self, s: &mut State, commit: Commit) {
        s.main.push(commit);
    }

    fn lander_push(&self, s: &mut State, c: Cand, parent: u8) {
        self.push(
            s,
            Commit {
                by: Pusher::Lander,
                parent,
                checked_base: c.base,
                revert: false,
                speculative: c.speculative,
                lanes: c.lanes,
                checked: Some((c.base_tree, c.lanes)),
            },
        );
        for l in self.lanes_of(c.lanes) {
            s.verdicts[l] = Some(Verdict::Landed);
        }
    }

    fn lower_half(mask: u8) -> u8 {
        let keep = mask.count_ones().div_ceil(2);
        let mut out = 0;
        let mut taken = 0;
        for b in 0..8 {
            if taken < keep && mask & (1 << b) != 0 {
                out |= 1 << b;
                taken += 1;
            }
        }
        out
    }
}

impl Model for Lander {
    type State = State;
    type Action = Action;

    fn init_states(&self) -> Vec<State> {
        let n = self.lanes as usize;
        let mut out = Vec::new();
        for code in 0..3usize.pow(n as u32) {
            let kinds = (0..n).map(|i| [Kind::Good, Kind::Bad, Kind::Latent][code / 3usize.pow(i as u32) % 3]).collect();
            out.push(State {
                kinds,
                pending: (1u8 << n) - 1,
                queue: 0,
                slots: Vec::new(),
                verdicts: vec![None; n],
                peer: Peer::Idle,
                main: Vec::new(),
                greens: BTreeSet::new(),
                ci: None,
                last_green: 0,
                cas_lost: false,
            });
        }
        out
    }

    fn actions(&self, s: &State, actions: &mut Vec<Action>) {
        for l in 0..self.lanes {
            if s.pending & (1 << l) != 0 {
                actions.push(Action::Enqueue(l));
            }
        }
        if s.slots.is_empty() && s.queue != 0 {
            actions.push(Action::Build);
        }
        if s.slots.len() == 1 && self.depth >= 2 && s.queue != 0 && s.slots[0].verdict != Some(false) {
            actions.push(Action::Speculate);
        }
        for (i, c) in s.slots.iter().enumerate() {
            if c.verdict.is_none() {
                actions.push(Action::Check(i));
            }
        }
        match s.slots.first().map(|c| c.verdict) {
            Some(Some(true)) => actions.push(Action::Push),
            Some(Some(false)) => actions.push(Action::Bisect),
            _ => {}
        }
        if self.variant == Variant::SpeculativePushUnchecked && s.slots.get(1).is_some_and(|c| c.verdict == Some(true)) {
            actions.push(Action::PushSpeculative);
        }
        match s.peer {
            Peer::Idle => actions.push(Action::PeerFetchCheck),
            Peer::Checked { .. } => actions.push(Action::PeerPush),
            Peer::Done(_) => {}
        }
        let tip = self.tip(s);
        if s.ci.map(|c| c.0) != Some(tip) {
            actions.push(Action::RunCi);
        }
        if s.ci == Some((tip, false)) {
            actions.push(Action::Sheriff);
        }
    }

    fn next_state(&self, last: &State, action: Action) -> Option<State> {
        let mut s = last.clone();
        let tip = self.tip(&s);
        match action {
            Action::Enqueue(l) => {
                s.pending &= !(1 << l);
                s.queue |= 1 << l;
            }
            Action::Build => {
                let base_tree = self.tree(&s.main, tip);
                let queue = std::mem::take(&mut s.queue);
                let lanes = self.admit(&mut s, base_tree, queue);
                if lanes != 0 {
                    s.slots.push(Cand { base: tip, base_tree, lanes, speculative: false, verdict: None });
                }
            }
            Action::Speculate => {
                let head = s.slots[0];
                let base_tree = head.base_tree | head.lanes;
                let queue = std::mem::take(&mut s.queue);
                let lanes = self.admit(&mut s, base_tree, queue);
                if lanes != 0 {
                    s.slots.push(Cand { base: head.base + 1, base_tree, lanes, speculative: true, verdict: None });
                }
            }
            Action::Check(i) => {
                let c = s.slots[i];
                let green = self.land_green(&s, c.base_tree | c.lanes);
                s.slots[i].verdict = Some(green);
                if green {
                    s.greens.insert((c.base_tree, c.lanes));
                }
            }
            Action::Push => {
                let head = s.slots[0];
                if tip == head.base {
                    s.slots.remove(0);
                    self.lander_push(&mut s, head, tip);
                } else {
                    for c in std::mem::take(&mut s.slots) {
                        s.queue |= c.lanes;
                    }
                    s.cas_lost = true;
                }
            }
            Action::PushSpeculative => {
                let spec = s.slots.remove(1);
                self.lander_push(&mut s, spec, tip);
            }
            Action::Bisect => {
                let head = s.slots[0];
                let spec = s.slots.get(1).map_or(0, |c| c.lanes);
                s.slots.clear();
                s.queue |= spec;
                if head.lanes.count_ones() == 1 {
                    for l in self.lanes_of(head.lanes) {
                        s.verdicts[l] = Some(Verdict::Culprit);
                    }
                } else {
                    let lo = Self::lower_half(head.lanes);
                    s.slots.push(Cand { lanes: lo, verdict: None, ..head });
                    s.queue |= head.lanes & !lo;
                }
            }
            Action::PeerFetchCheck => {
                let tree = self.tree(&s.main, tip);
                if self.conflicting(self.peer_bit(), tree) != 0 {
                    s.peer = Peer::Done(Verdict::Conflict);
                } else {
                    s.greens.insert((tree, self.peer_bit()));
                    s.peer = Peer::Checked { base: tip, tree };
                }
            }
            Action::PeerPush => {
                let Peer::Checked { base, tree } = s.peer else { return None };
                if tip == base {
                    let lanes = self.peer_bit();
                    self.push(
                        &mut s,
                        Commit { by: Pusher::Peer, parent: tip, checked_base: base, revert: false, speculative: false, lanes, checked: Some((tree, lanes)) },
                    );
                    s.peer = Peer::Done(Verdict::Landed);
                } else {
                    s.peer = Peer::Idle;
                }
            }
            Action::RunCi => {
                let green = self.full_green(&s, self.tree(&s.main, tip));
                s.ci = Some((tip, green));
                if green {
                    s.last_green = tip;
                }
            }
            Action::Sheriff => {
                let tree = self.tree(&s.main, tip);
                let range = s.main[s.last_green as usize..].iter().filter(|c| !c.revert).fold(0, |m, c| m | c.lanes) & tree;
                let latent = self.lanes_of(range).filter(|&l| s.kinds[l] == Kind::Latent).fold(0, |m, l| m | (1 << l));
                let reverted = match self.variant {
                    Variant::SheriffRevertsWholeRange => range,
                    _ if range.count_ones() == 1 => range,
                    _ => latent,
                };
                self.push(
                    &mut s,
                    Commit { by: Pusher::Sheriff, parent: tip, checked_base: tip, revert: true, speculative: false, lanes: reverted, checked: None },
                );
                for l in self.lanes_of(reverted) {
                    s.verdicts[l] = Some(Verdict::Culprit);
                }
            }
        }
        Some(s)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        let all = vec![
            Property::<Self>::always(EXACTNESS, |m, s| {
                s.main.iter().filter(|c| !c.revert).all(|c| {
                    let checked = (m.tree(&s.main, c.parent), c.lanes);
                    c.checked == Some(checked) && s.greens.contains(&checked)
                })
            }),
            Property::<Self>::eventually(NO_LOST_LANE, |_, s| {
                s.pending == 0 && s.verdicts.iter().all(Option::is_some) && matches!(s.peer, Peer::Done(_))
            }),
            Property::<Self>::always(NO_DOUBLE_LANDING, |m, s| {
                (0..=m.lanes).all(|l| s.main.iter().filter(|c| !c.revert && c.lanes & (1 << l) != 0).count() <= 1)
            }),
            Property::<Self>::always(CULPRIT_ONLY, |m, s| {
                let bad_landed = s.main.iter().filter(|c| !c.revert).any(|c| m.lanes_of(c.lanes).any(|l| s.kinds[l] == Kind::Bad));
                let good_ejected = (0..m.lanes as usize).any(|l| s.kinds[l] == Kind::Good && s.verdicts[l] == Some(Verdict::Culprit));
                !bad_landed && !good_ejected
            }),
            Property::<Self>::always(CAS_SAFETY, |_, s| {
                s.main.iter().enumerate().all(|(i, c)| c.parent as usize == i && c.parent == c.checked_base)
            }),
            Property::<Self>::sometimes(REACH_SPECULATIVE_LANDED, |_, s| s.main.iter().any(|c| c.speculative && c.by == Pusher::Lander)),
            Property::<Self>::sometimes(REACH_CAS_LOST, |_, s| s.cas_lost),
            Property::<Self>::sometimes(REACH_REVERT, |_, s| s.main.iter().any(|c| c.by == Pusher::Sheriff)),
            Property::<Self>::sometimes(REACH_CULPRIT, |m, s| {
                (0..m.lanes as usize).any(|l| s.kinds[l] == Kind::Bad && s.verdicts[l] == Some(Verdict::Culprit))
                    && (0..m.lanes as usize).filter(|&l| s.kinds[l] == Kind::Good && s.verdicts[l] == Some(Verdict::Landed)).count() >= 2
            }),
            Property::<Self>::sometimes(REACH_CONFLICT, |_, s| {
                s.verdicts.contains(&Some(Verdict::Conflict)) || s.peer == Peer::Done(Verdict::Conflict)
            }),
        ];
        all.into_iter().filter(|p| self.only.is_none_or(|o| o == p.name)).collect()
    }
}

fn check(model: Lander, property: &'static str) -> (usize, Option<String>) {
    let model = Lander { only: Some(property), ..model };
    let started = Instant::now();
    let checker = model.checker().threads(std::thread::available_parallelism().map_or(1, |n| n.get())).spawn_bfs().join();
    let states = checker.unique_state_count();
    let trace = checker.discovery(property).map(|path| {
        let steps = path.into_vec();
        let first = &steps[0].0;
        let last = &steps[steps.len() - 1].0;
        let mut out = format!("    lanes {:?}, peer lane Good\n", first.kinds);
        for (_, action) in &steps {
            if let Some(a) = action {
                out.push_str(&format!("    {a:?}\n"));
            }
        }
        out.push_str(&format!("    main {:?}\n    verdicts {:?}\n", last.main, last.verdicts));
        out
    });
    println!("{property}: {states} states explored in {:.1?}", started.elapsed());
    (states, trace)
}

#[test]
fn plan_design_holds_every_property() {
    for lanes in [3] {
        println!("bounds: {lanes} lander lanes, 1 peer-host lane, speculative depth 2");
        for property in PROPERTIES {
            let (_, trace) = check(Lander::plan(lanes), property);
            assert!(trace.is_none(), "{property} fails:\n{}", trace.unwrap_or_default());
        }
    }
}

#[test]
fn model_reaches_every_interesting_case() {
    for property in REACHABLE {
        let (_, trace) = check(Lander::plan(3), property);
        assert!(trace.is_some(), "the model never reaches: {property}");
    }
}

#[test]
fn speculative_push_without_landed_check_is_caught() {
    let model = Lander { variant: Variant::SpeculativePushUnchecked, ..Lander::plan(3) };
    let (_, trace) = check(model, EXACTNESS);
    let trace = trace.expect("variant (a) must break exactness");
    println!("counterexample, speculative N+1 pushes without checking N landed:\n{trace}");
}

#[test]
fn sheriff_reverting_a_multi_landing_range_is_caught() {
    let model = Lander { variant: Variant::SheriffRevertsWholeRange, ..Lander::plan(3) };
    let (_, trace) = check(model, CULPRIT_ONLY);
    let trace = trace.expect("variant (b) must break culprit-only ejection");
    println!("counterexample, sheriff reverts every landing of a red range:\n{trace}");
}
