//! The lander's after-landing state machines (north#757): each landed head's after-landing run, each repository's
//! red-main episode and each queue's base-broken pause hold one record in the lander host's lifecycle store, so a
//! lander killed mid-run, an alarm that never went out or a pause nobody lifted shows in `north health`.
use std::path::Path;
use std::time::Duration;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Delivery, Effect, Leased, Lifecycle, Record, Refusal, State, Store};

const MINUTE: u64 = 60;
const HOUR: u64 = 3600;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Run {
    /// Landed, waiting for its after-landing run.
    Journaled,
    /// Its checks run, held by the lander that started or adopted them.
    Running,
    /// Its last run reached no verdict; it runs again after the retry delay.
    Unsettled,
    /// Red, the lander searches for the culprit lane.
    Searching,
    /// The lander holding it is gone; the next lander adopts or reruns it.
    Abandoned,
    /// A verdict was reached and acted on.
    Retired,
}

impl State for Run {
    const ALL: &'static [Self] = &[Self::Journaled, Self::Running, Self::Unsettled, Self::Searching, Self::Abandoned, Self::Retired];

    fn name(self) -> &'static str {
        match self {
            Self::Journaled => "journaled",
            Self::Running => "running",
            Self::Unsettled => "unsettled",
            Self::Searching => "searching",
            Self::Abandoned => "abandoned",
            Self::Retired => "retired",
        }
    }
}

/// One landed head's after-landing run; the instance is the head.
pub struct AfterLandingRun;

impl Lifecycle for AfterLandingRun {
    const NAME: &'static str = "after-landing-run";
    type State = Run;

    fn legal(from: Option<Run>, to: Run) -> bool {
        use Run::{Abandoned, Journaled, Retired, Running, Searching, Unsettled};
        matches!(
            (from, to),
            (None, Journaled)
                | (Some(Journaled | Unsettled | Abandoned), Running)
                | (Some(Journaled | Running | Unsettled | Abandoned), Journaled)
                | (Some(Running), Unsettled | Searching)
                | (Some(_), Retired)
        )
    }

    fn dwell(state: Run) -> Option<Duration> {
        match state {
            Run::Journaled | Run::Searching => Some(Duration::from_secs(2 * HOUR)),
            Run::Running => Some(Duration::from_secs(4 * HOUR)),
            Run::Unsettled => Some(Duration::from_secs(HOUR)),
            Run::Abandoned => Some(Duration::from_secs(30 * MINUTE)),
            Run::Retired => None,
        }
    }

    fn orphan(state: Run) -> Option<Run> {
        matches!(state, Run::Running | Run::Searching).then_some(Run::Abandoned)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Main {
    Green,
    /// A red verdict opened the episode; the proxy has not been told yet.
    Opened,
    /// The proxy was told main is red.
    Red,
    /// A green run covered the failing checks; the proxy has not been told yet.
    Cleared,
}

impl State for Main {
    const ALL: &'static [Self] = &[Self::Green, Self::Opened, Self::Red, Self::Cleared];

    fn name(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Opened => "opened",
            Self::Red => "red",
            Self::Cleared => "cleared",
        }
    }
}

/// One repository's red-main episode; the instance is the repository.
pub struct RedMain;

impl Lifecycle for RedMain {
    const NAME: &'static str = "red-main";
    type State = Main;

    fn legal(from: Option<Main>, to: Main) -> bool {
        matches!((from, to), (None | Some(Main::Green | Main::Cleared), Main::Opened) | (Some(Main::Opened), Main::Red) | (Some(Main::Opened | Main::Red), Main::Cleared) | (Some(Main::Cleared), Main::Green))
    }

    fn dwell(state: Main) -> Option<Duration> {
        matches!(state, Main::Opened | Main::Cleared).then_some(Duration::from_secs(15 * MINUTE))
    }

    fn orphan(_: Main) -> Option<Main> {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pause {
    Clear,
    /// Main's check environment is broken; the alert has not gone out.
    Paused,
    /// Paused and alerted.
    Alerted,
}

impl State for Pause {
    const ALL: &'static [Self] = &[Self::Clear, Self::Paused, Self::Alerted];

    fn name(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Paused => "paused",
            Self::Alerted => "alerted",
        }
    }
}

/// One queue's base-broken pause; the instance is the queue's directory name. Each raise moves the record again, so
/// its `since` is the last raise and new candidates wait `RETRY` from it.
pub struct BaseBroken;

impl BaseBroken {
    pub const RETRY: Duration = Duration::from_secs(300);
}

impl Lifecycle for BaseBroken {
    const NAME: &'static str = "base-broken";
    type State = Pause;

    fn legal(from: Option<Pause>, to: Pause) -> bool {
        matches!((from, to), (None | Some(Pause::Clear | Pause::Alerted), Pause::Paused) | (Some(Pause::Paused), Pause::Paused | Pause::Alerted | Pause::Clear) | (Some(Pause::Alerted), Pause::Alerted | Pause::Clear))
    }

    fn dwell(_: Pause) -> Option<Duration> {
        None
    }

    fn orphan(_: Pause) -> Option<Pause> {
        None
    }
}

/// This host's lifecycle store, or why it cannot be opened, printed once by the caller.
pub fn host() -> Result<Store, String> {
    Store::host().map_err(|error| format!("north land: the lifecycle store cannot be opened: {error}"))
}

/// Moves `instance` of `L` to `to`, printing a refusal: the lander keeps landing when a record disagrees, and the
/// refused move is logged where `north health` reports it.
pub fn moved<L: Lifecycle>(store: &mut Store, instance: &str, to: L::State, cause: &str) -> Option<Record<L::State>> {
    match store.transition::<L>(instance, to, cause, Timestamp::now()) {
        Ok(Ok(record)) => Some(record),
        Ok(Err(Refusal::Illegal { from, to })) => {
            println!("north land: {} {instance} may not move from {} to {}: {cause}", L::NAME, from.map_or("nothing", State::name), to.name());
            None
        }
        Ok(Err(Refusal::Held { state, owner })) => {
            println!("north land: {} {instance} is {} under {owner}, so it stays: {cause}", L::NAME, state.name());
            None
        }
        Err(error) => {
            println!("north land: {} {instance} could not move to {}: {error}", L::NAME, to.name());
            None
        }
    }
}

pub fn current<L: Lifecycle>(store: &Store, instance: &str) -> Option<Record<L::State>> {
    store.current::<L>(instance).unwrap_or_else(|error| {
        println!("north land: {} {instance} is unreadable: {error}", L::NAME);
        None
    })
}

/// How many runs of `head` reached no verdict, counted from its logged moves, so a lander restart keeps the count.
pub fn misses(store: &Store, head: &str) -> usize {
    store.history(AfterLandingRun::NAME, head).map_or(0, |moves| moves.iter().filter(|logged| !logged.refused && logged.to == Run::Unsettled.name()).count())
}

#[cfg(test)]
#[path = "after_landing_lifecycle_tests.rs"]
mod tests;

/// How long a lander holds a head's after-landing run before `north health` reports it: its Journaled and Running
/// dwells together.
const RUN_HOLD: Duration = Duration::from_secs(6 * HOUR);

/// The outbox kind of one queue's after-landing runs, so a lander takes only its own queue's.
fn run_kind(queue: &Path) -> String {
    format!("{} {}", AfterLandingRun::NAME, queue.display())
}

fn run_key(queue: &Path, head: &str) -> String {
    format!("{}/{head}", run_kind(queue))
}

/// Moves `head` to Journaled with its after-landing `run` committed in the same transaction, so a lander killed in
/// between neither loses the run nor journals one it never recorded (north#862).
pub fn journal(store: &mut Store, queue: &Path, head: &str, cause: &str, run: serde_json::Value) -> Result<(), String> {
    let effect = Effect { kind: run_kind(queue), key: run_key(queue, head), payload: run, delivery: Delivery::ReplaySafe };
    match store.transition_with_effects::<AfterLandingRun>(head, Run::Journaled, cause, Timestamp::now(), None, &[effect]) {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(Refusal::Illegal { from, .. })) => Err(format!("{head} may not be journaled from {}", from.map_or("nothing", State::name))),
        Ok(Err(Refusal::Held { state, owner })) => Err(format!("{head} is {} under {owner}", state.name())),
        Err(error) => Err(error.to_string()),
    }
}

/// Every journaled after-landing run of `queue` no live lander holds, leased to this one until its verdict.
pub fn take(store: &mut Store, queue: &Path) -> Result<Vec<Leased>, String> {
    let kind = run_kind(queue);
    for record in store.retire_gone::<AfterLandingRun>(Timestamp::now()).map_err(|error| error.to_string())? {
        println!("north land: after-landing-run {} lost its holder and is now {}", record.instance, record.state.name());
    }
    std::iter::from_fn(|| store.lease(&kind, Timestamp::now(), RUN_HOLD).transpose()).collect::<Result<_, _>>().map_err(|error| error.to_string())
}

/// Settles `head`'s journaled run once its verdict was acted on, or its record says it already was.
pub fn settled(store: &mut Store, queue: &Path, head: &str) {
    if let Err(error) = store.delivered(&run_key(queue, head), Timestamp::now()) {
        println!("north land: the journaled after-landing run of {head} could not be settled: {error}");
    }
}

/// The heads of `queue` whose journaled run awaits its verdict, oldest first.
pub fn journaled(store: &Store, queue: &Path) -> Vec<String> {
    store.open_effects(&run_kind(queue)).unwrap_or_else(|error| {
        println!("north land: the journaled after-landing runs are unreadable: {error}");
        Vec::new()
    })
}

/// Hands `heads`' journaled runs back for the next lander, as a lander that stops following does.
pub fn hand_back<'a>(store: &mut Store, queue: &Path, heads: impl IntoIterator<Item = &'a str>) {
    for head in heads {
        if let Err(error) = store.release(&run_key(queue, head), Timestamp::now()) {
            println!("north land: the journaled after-landing run of {head} could not be handed back: {error}");
        }
    }
}
