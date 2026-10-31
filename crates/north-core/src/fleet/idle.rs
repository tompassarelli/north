use std::time::Duration;

use super::silent::SILENT_AFTER;
use super::wait::WAKE_AFTER;

/// How long a session with a registered run stays up after its turn ends, so a revival message sent to it starts a turn before it parks again.
const REGISTERED_PARK_AFTER: Duration = Duration::from_secs(30);

/// How long a session holds off parking for a run it detached; past it the run is registered as a wait instead.
const DETACHED_HOLD: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub silent_after: Duration,
    pub rollover_after: Duration,
    pub cache_lifetime: Duration,
    pub rollover_min_tokens: u64,
    pub finished_exit_after: Duration,
    pub wake_after: [Duration; 3],
    pub detached_hold: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self { silent_after: SILENT_AFTER, rollover_after: Duration::from_secs(55 * 60), cache_lifetime: Duration::from_secs(60 * 60), rollover_min_tokens: 200_000, finished_exit_after: Duration::from_secs(60 * 60), wake_after: WAKE_AFTER, detached_hold: DETACHED_HOLD }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Session {
    pub idle: Duration,
    pub in_flight: bool,
    pub context: u64,
    pub rolled_over: bool,
    pub finished: bool,
    pub wakes: Option<usize>,
    pub registered: bool,
    pub detached: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Wait,
    Compact,
    Park,
    Exit,
    Wake,
    Stall,
}

impl Policy {
    pub fn step(&self, session: Session, park_after: Option<Duration>) -> Step {
        let parks = park_after.is_some_and(|limit| session.idle >= limit);
        if session.finished {
            let done = session.idle >= self.finished_exit_after || parks && !session.in_flight;
            return if done { Step::Exit } else { Step::Wait };
        }
        if session.in_flight {
            return Step::Wait;
        }
        if session.registered && session.idle >= REGISTERED_PARK_AFTER {
            return Step::Park;
        }
        if session.detached {
            let held = session.idle < self.detached_hold;
            return if held { self.rollover(session).unwrap_or(Step::Wait) } else { Step::Park };
        }
        if let Some(wakes) = session.wakes {
            return self.wake_step(wakes, session.idle);
        }
        if let Some(step) = self.rollover(session) {
            return step;
        }
        if parks { Step::Park } else { Step::Wait }
    }

    fn rollover(&self, session: Session) -> Option<Step> {
        let due = session.context >= self.rollover_min_tokens && !session.rolled_over && session.idle < self.cache_lifetime;
        due.then(|| if session.idle >= self.rollover_after { Step::Compact } else { Step::Wait })
    }

    fn wake_step(&self, wakes: usize, idle: Duration) -> Step {
        match self.wake_after.get(wakes) {
            Some(after) if idle >= *after => Step::Wake,
            Some(_) => Step::Wait,
            None => Step::Stall,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARK: Option<Duration> = Some(Duration::from_secs(10 * 60));

    fn minutes(minutes: u64) -> Duration {
        Duration::from_secs(minutes * 60)
    }

    fn step(session: Session) -> Step {
        Policy::default().step(session, PARK)
    }

    #[test]
    fn an_idle_session_at_200k_or_more_compacts_at_55_minutes_while_its_cache_is_warm_and_not_before() {
        let large = Session { context: 200_000, ..Session::default() };
        assert_eq!(step(Session { idle: minutes(54), ..large }), Step::Wait, "held past its park time until the rollover");
        assert_eq!(step(Session { idle: minutes(55), ..large }), Step::Compact);
        assert_eq!(step(Session { idle: minutes(55), in_flight: true, ..large }), Step::Wait, "never mid-turn or with a task out");
        assert_eq!(Policy::default().step(Session { idle: minutes(55), ..large }, None), Step::Compact, "a host that never parks still rolls over");
        assert_eq!(step(Session { idle: minutes(60), ..large }), Step::Park, "past the cache lifetime a compaction would pay the cold rewrite it saves");
    }

    #[test]
    fn a_session_under_200k_or_already_rolled_over_is_never_compacted_and_parks_as_before() {
        let small = Session { context: 199_999, ..Session::default() };
        assert_eq!(step(Session { idle: minutes(9), ..small }), Step::Wait);
        assert_eq!(step(Session { idle: minutes(10), ..small }), Step::Park);
        assert_eq!(step(Session { idle: minutes(55), ..small }), Step::Park);
        let rolled = Session { context: 250_000, rolled_over: true, idle: minutes(10), ..Session::default() };
        assert_eq!(step(rolled), Step::Park, "one rollover per wake, even when the summary stays large");
    }

    #[test]
    fn a_finished_worker_exits_at_its_park_time_and_by_60_idle_minutes_even_with_a_task_still_out() {
        let finished = Session { finished: true, context: 400_000, ..Session::default() };
        assert_eq!(step(Session { idle: minutes(9), ..finished }), Step::Wait);
        assert_eq!(step(Session { idle: minutes(10), ..finished }), Step::Exit, "exits instead of parking, and is never compacted");
        assert_eq!(step(Session { idle: minutes(59), in_flight: true, ..finished }), Step::Wait);
        assert_eq!(step(Session { idle: minutes(60), in_flight: true, ..finished }), Step::Exit);
        assert_eq!(Policy::default().step(Session { idle: minutes(60), ..finished }, Some(minutes(600))), Step::Exit, "a longer idle limit stops at 60");
    }

    #[test]
    fn a_session_with_a_registered_run_parks_at_once_instead_of_being_woken_or_held_to_the_idle_limit() {
        let registered = Session { registered: true, wakes: Some(0), idle: Duration::from_secs(30), ..Session::default() };
        assert_eq!(step(registered), Step::Park);
        assert_eq!(step(Session { idle: Duration::from_secs(5), ..registered }), Step::Wait, "a revival message gets 30 s to start a turn first");
        assert_eq!(step(Session { in_flight: true, ..registered }), Step::Wait, "never mid-turn");
        assert_eq!(step(Session { finished: true, idle: minutes(10), ..registered }), Step::Exit, "a final report still exits it");
        assert_eq!(Policy::default().step(registered, None), Step::Park, "whatever the idle limit");
    }

    #[test]
    fn a_session_with_a_detached_run_holds_past_its_idle_limit_unwoken_until_the_hold_then_parks() {
        let detached = Session { detached: true, wakes: Some(0), ..Session::default() };
        assert_eq!(step(Session { idle: minutes(35), ..detached }), Step::Wait, "a 35-minute run outlasts the 10-minute idle limit and every wake");
        assert_eq!(step(Session { idle: minutes(119), ..detached }), Step::Wait);
        assert_eq!(step(Session { idle: minutes(120), ..detached }), Step::Park, "past the hold the run is registered and the session parks");
        assert_eq!(step(Session { idle: minutes(55), context: 250_000, ..detached }), Step::Compact, "a held session still rolls over");
        assert_eq!(step(Session { idle: minutes(30), registered: true, ..detached }), Step::Park, "a run the worker registered parks it as before");
        assert_eq!(step(Session { idle: minutes(10), finished: true, ..detached }), Step::Exit, "a final report still exits it");
    }

    #[test]
    fn a_session_that_ended_on_a_wait_is_woken_at_5_15_and_45_minutes_then_stalls_instead_of_parking() {
        let waiting = |idle, wakes| Session { idle: minutes(idle), wakes: Some(wakes), ..Session::default() };
        assert_eq!(step(waiting(4, 0)), Step::Wait, "held past its park time while a wait is open");
        assert_eq!(step(waiting(5, 0)), Step::Wake);
        assert_eq!(step(waiting(14, 1)), Step::Wait);
        assert_eq!(step(waiting(15, 1)), Step::Wake);
        assert_eq!(step(waiting(45, 2)), Step::Wake);
        assert_eq!(step(waiting(1, 3)), Step::Stall, "after the last wake the parent is told, whatever the idle time");
        assert_eq!(step(Session { in_flight: true, ..waiting(60, 0) }), Step::Wait, "never mid-turn");
        assert_eq!(step(Session { finished: true, ..waiting(10, 0) }), Step::Exit, "a final report ends the wait");
    }
}
