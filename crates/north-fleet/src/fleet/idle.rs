use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub eta_minute: Duration,
    pub rollover_after: Duration,
    pub cache_lifetime: Duration,
    pub rollover_min_tokens: u64,
    pub finished_exit_after: Duration,
    pub registered_park_after: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self { eta_minute: Duration::from_secs(60), rollover_after: Duration::from_secs(55 * 60), cache_lifetime: Duration::from_secs(60 * 60), rollover_min_tokens: 200_000, finished_exit_after: Duration::from_secs(60 * 60), registered_park_after: Duration::from_secs(30) }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Session {
    pub idle: Duration,
    pub in_flight: bool,
    pub turn_open: bool,
    pub context: u64,
    pub rolled_over: bool,
    pub finished: bool,
    pub registered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    Wait,
    Compact,
    Park,
    Exit,
}

impl Policy {
    pub fn step(&self, session: Session, park_after: Option<Duration>) -> Step {
        let parks = park_after.is_some_and(|limit| session.idle >= limit);
        if session.finished {
            let done = session.idle >= self.finished_exit_after || parks && !session.turn_open;
            return if done { Step::Exit } else { Step::Wait };
        }
        if session.in_flight {
            return Step::Wait;
        }
        if session.registered && session.idle >= self.registered_park_after {
            return Step::Park;
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
}

