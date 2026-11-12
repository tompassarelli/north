use std::cell::Cell;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use north_base::error::NorthResult;
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, Liveness, Move, Owner, Record, State, Store};

/// The scope's processes as host and pid: two share a host, so each can tell whether the other runs, and the third
/// cannot be seen from there.
pub(crate) const ACTORS: [(&str, u32); 3] = [("a", 1), ("a", 2), ("b", 3)];
pub(crate) const START: Situation = Situation { state: None, holder: None, dead: false, overdue: false };
const START_MS: i64 = 1_700_000_000_000;
const CAUSE: &str = "explored";
static WORLDS: AtomicU64 = AtomicU64::new(0);

fn actor(index: usize) -> Owner {
    let (host, pid) = ACTORS[index];
    Owner { host: host.to_owned(), pid, started: u64::from(pid), booted_ms: 1 }
}

pub(crate) fn label(index: usize) -> String {
    format!("{}{}", ACTORS[index].0, ACTORS[index].1)
}

struct Watch {
    host: &'static str,
    dead: Rc<Cell<Option<usize>>>,
}

impl Liveness for Watch {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        (owner.host == self.host).then(|| self.dead.get().is_none_or(|dead| actor(dead) != *owner))
    }
}

/// All the store's next answer depends on: the instance's state, the process holding it, whether that process is
/// dead and whether the state's dwell has run out.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Situation {
    pub(crate) state: Option<usize>,
    pub(crate) holder: Option<usize>,
    pub(crate) dead: bool,
    pub(crate) overdue: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    Move { by: usize, to: usize, holder: usize },
    Settle { by: usize },
    Die,
    Late,
    Expire { by: usize },
}

impl Event {
    pub(crate) fn by(self) -> Option<usize> {
        match self {
            Self::Move { by, .. } | Self::Settle { by } | Self::Expire { by } => Some(by),
            Self::Die | Self::Late => None,
        }
    }

    pub(crate) fn told<L: Lifecycle>(self) -> String {
        match self {
            Self::Move { by, to, holder } if by == holder => format!("{} moves it to {}", label(by), L::State::ALL[to].name()),
            Self::Move { by, to, holder } => format!("{} moves it to {} for {}", label(by), L::State::ALL[to].name(), label(holder)),
            Self::Settle { by } => format!("{} settles it", label(by)),
            Self::Die => "the process holding it dies".to_owned(),
            Self::Late => "its dwell runs out".to_owned(),
            Self::Expire { by } => format!("{} sweeps its deadline", label(by)),
        }
    }
}

pub(crate) fn told<L: Lifecycle>(path: &[Event]) -> Vec<String> {
    path.iter().map(|event| event.told::<L>()).collect()
}

/// One ordering played on a fresh instance: where it ended, the instance's whole log and how much of it the last
/// event wrote.
pub(crate) struct Played<S> {
    pub(crate) situation: Situation,
    pub(crate) log: Vec<Move>,
    pub(crate) record: Option<Record<S>>,
    seen: usize,
}

impl<S> Played<S> {
    pub(crate) fn fresh(&self) -> &[Move] {
        &self.log[self.seen..]
    }
}

pub(crate) struct World<L: Lifecycle> {
    stores: Vec<Store>,
    dead: Rc<Cell<Option<usize>>>,
    plays: u64,
    machine: PhantomData<L>,
}

impl<L: Lifecycle> World<L> {
    pub(crate) fn open() -> NorthResult<Self> {
        let memory = format!("file:north-machine-model-{}-{}?mode=memory&cache=shared", std::process::id(), WORLDS.fetch_add(1, Ordering::Relaxed));
        let dead = Rc::new(Cell::new(None));
        let stores = (0..ACTORS.len())
            .map(|index| Store::open_as(Path::new(&memory), actor(index), Box::new(Watch { host: ACTORS[index].0, dead: Rc::clone(&dead) })))
            .collect::<NorthResult<_>>()?;
        Ok(Self { stores, dead, plays: 0, machine: PhantomData })
    }

    pub(crate) fn play(&mut self, path: &[Event]) -> NorthResult<Played<L::State>> {
        self.plays += 1;
        self.dead.set(None);
        let instance = format!("i{}", self.plays);
        let (mut now, mut seen) = (START_MS, 0);
        for (position, event) in path.iter().enumerate() {
            if position + 1 == path.len() {
                seen = self.stores[0].history(L::NAME, &instance)?.len();
            }
            now += 1;
            self.apply(&instance, *event, &mut now)?;
        }
        let log = self.stores[0].history(L::NAME, &instance)?;
        let record = self.stores[0].current::<L>(&instance)?;
        let holder = self.holder(&instance)?;
        let situation = Situation {
            state: record.as_ref().and_then(|record| L::State::ALL.iter().position(|state| *state == record.state)),
            holder,
            dead: holder.is_some() && self.dead.get() == holder,
            overdue: record.as_ref().and_then(|record| record.due).is_some_and(|due| now > due.unix_millis()),
        };
        Ok(Played { situation, log, record, seen })
    }

    fn apply(&mut self, instance: &str, event: Event, now: &mut i64) -> NorthResult<()> {
        let at = Timestamp::from_unix_millis(*now);
        match event {
            Event::Move { by, to, holder } => drop(self.stores[by].transition_for::<L>(instance, L::State::ALL[to], CAUSE, at, &actor(holder))?),
            Event::Settle { by } => drop(self.stores[by].settle::<L>(instance, at)?),
            Event::Expire { by } => drop(self.stores[by].expire::<L>(at)?),
            Event::Die => self.dead.set(self.holder(instance)?),
            Event::Late => *now = self.stores[0].current::<L>(instance)?.and_then(|record| record.due).map_or(*now, |due| due.unix_millis() + 1),
        }
        if self.dead.get().is_some() && self.holder(instance)? != self.dead.get() {
            self.dead.set(None);
        }
        Ok(())
    }

    fn holder(&self, instance: &str) -> NorthResult<Option<usize>> {
        let owner = self.stores[0].current::<L>(instance)?.and_then(|record| record.owner);
        Ok(owner.and_then(|owner| (0..ACTORS.len()).find(|index| actor(*index) == owner)))
    }
}

/// Two instances of one machine, played together so a promise between them can be stated: the first through every
/// move, the second attempting to start at each of the first's situations.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PairSituation {
    pub(crate) sides: [Situation; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PairEvent {
    /// One instance's move.
    One { side: usize, event: Event },
    /// Two launches claiming one name at the same time, each into the state at its index.
    Race { left: usize, right: usize },
}

/// One ordering of two instances: where each ended, and the transitions the last event made, one per instance.
pub(crate) struct PairPlayed<S> {
    pub(crate) sides: [Situation; 2],
    pub(crate) moved: [Option<Played<S>>; 2],
}

pub(crate) struct PairWorld<L: Lifecycle> {
    stores: Vec<Store>,
    dead: Rc<Cell<Option<usize>>>,
    plays: u64,
    machine: PhantomData<L>,
}

impl<L: Lifecycle> PairWorld<L> {
    pub(crate) fn open() -> NorthResult<Self> {
        let memory = format!("file:north-machine-model-pair-{}-{}?mode=memory&cache=shared", std::process::id(), WORLDS.fetch_add(1, Ordering::Relaxed));
        let dead = Rc::new(Cell::new(None));
        let stores = (0..ACTORS.len())
            .map(|index| Store::open_as(Path::new(&memory), actor(index), Box::new(Watch { host: ACTORS[index].0, dead: Rc::clone(&dead) })))
            .collect::<NorthResult<_>>()?;
        Ok(Self { stores, dead, plays: 0, machine: PhantomData })
    }

    pub(crate) fn play(&mut self, path: &[PairEvent]) -> NorthResult<PairPlayed<L::State>> {
        self.plays += 1;
        self.dead.set(None);
        let instances = [format!("l{}", self.plays), format!("r{}", self.plays)];
        let (mut now, mut seen) = (START_MS, [0_usize; 2]);
        let mut moved: [Option<Played<L::State>>; 2] = [None, None];
        for (position, step) in path.iter().enumerate() {
            now += 1;
            match *step {
                PairEvent::One { side, event } => {
                    if position + 1 == path.len() {
                        seen[side] = self.stores[side].history(L::NAME, &instances[side])?.len();
                    }
                    self.apply(&instances[side], side, event, &mut now)?;
                }
                PairEvent::Race { left, right } => {
                    if position + 1 == path.len() {
                        (seen[0], seen[1]) = (self.stores[0].history(L::NAME, &instances[0])?.len(), self.stores[1].history(L::NAME, &instances[1])?.len());
                    }
                    self.apply(&instances[0], 0, Event::Move { by: 0, to: left, holder: 0 }, &mut now)?;
                    self.apply(&instances[1], 1, Event::Move { by: 1, to: right, holder: 1 }, &mut now)?;
                }
            }
        }
        let sides = [self.situation(&instances[0], now)?, self.situation(&instances[1], now)?];
        if let Some(PairEvent::One { side, .. }) = path.last() {
            moved[*side] = Some(self.played(&instances[*side], *side, seen[*side], sides[*side])?);
        } else if path.last().is_some() {
            (moved[0], moved[1]) = (Some(self.played(&instances[0], 0, seen[0], sides[0])?), Some(self.played(&instances[1], 1, seen[1], sides[1])?));
        }
        Ok(PairPlayed { sides, moved })
    }

    fn played(&self, instance: &str, side: usize, seen: usize, situation: Situation) -> NorthResult<Played<L::State>> {
        let log = self.stores[side].history(L::NAME, instance)?;
        let record = self.stores[side].current::<L>(instance)?;
        Ok(Played { situation, log, record, seen })
    }

    fn situation(&self, instance: &str, now: i64) -> NorthResult<Situation> {
        let record = self.stores[0].current::<L>(instance)?;
        let holder = self.holder(instance)?;
        Ok(Situation {
            state: record.as_ref().and_then(|record| L::State::ALL.iter().position(|state| *state == record.state)),
            holder,
            dead: holder.is_some() && self.dead.get() == holder,
            overdue: record.as_ref().and_then(|record| record.due).is_some_and(|due| now > due.unix_millis()),
        })
    }

    fn apply(&mut self, instance: &str, side: usize, event: Event, now: &mut i64) -> NorthResult<()> {
        let at = Timestamp::from_unix_millis(*now);
        match event {
            Event::Move { by, to, holder } => drop(self.stores[by].transition_for::<L>(instance, L::State::ALL[to], CAUSE, at, &actor(holder))?),
            Event::Settle { by } => drop(self.stores[by].settle::<L>(instance, at)?),
            Event::Expire { by } => drop(self.stores[by].expire::<L>(at)?),
            Event::Die => self.dead.set(self.holder(instance)?),
            Event::Late => *now = self.stores[side].current::<L>(instance)?.and_then(|record| record.due).map_or(*now, |due| due.unix_millis() + 1),
        }
        Ok(())
    }

    fn holder(&self, instance: &str) -> NorthResult<Option<usize>> {
        let owner = self.stores[0].current::<L>(instance)?.and_then(|record| record.owner);
        Ok(owner.and_then(|owner| (0..ACTORS.len()).find(|index| actor(*index) == owner)))
    }
}

/// Everything that can happen next: each live process tries every state, for every holder where the state is one a
/// process holds, and settles the instance; the holder dies; the dwell runs out; a live process sweeps an overdue state
/// that expires.
pub(crate) fn events<L: Lifecycle>(situation: &Situation) -> Vec<Event> {
    let states = L::State::ALL;
    let mut events = Vec::new();
    for by in (0..ACTORS.len()).filter(|by| !(situation.dead && situation.holder == Some(*by))) {
        for (to, state) in states.iter().enumerate() {
            let holders = if L::orphan(*state).is_some() { 0..ACTORS.len() } else { by..by + 1 };
            events.extend(holders.map(|holder| Event::Move { by, to, holder }));
        }
        events.push(Event::Settle { by });
    }
    if situation.holder.is_some() && !situation.dead {
        events.push(Event::Die);
    }
    if situation.state.is_some_and(|state| L::dwell(states[state]).is_some()) && !situation.overdue {
        events.push(Event::Late);
    }
    if situation.overdue && situation.state.is_some_and(|state| L::expire(states[state]).is_some()) {
        events.extend((0..ACTORS.len()).filter(|by| !(situation.dead && situation.holder == Some(*by))).map(|by| Event::Expire { by }));
    }
    events
}
