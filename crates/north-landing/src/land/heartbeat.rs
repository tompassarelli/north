use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::lander::Lander;

pub const STUCK: Duration = Duration::from_secs(30);
pub const REPEAT: Duration = Duration::from_secs(300);
const LOOK: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Between,
    Settle,
    Jobs,
    Prepare,
    Build,
    Check,
    Culprit,
    Push,
    Finish,
    Speculate,
    Watchdog,
}

const STEPS: [Step; 11] = [Step::Between, Step::Settle, Step::Jobs, Step::Prepare, Step::Build, Step::Check, Step::Culprit, Step::Push, Step::Finish, Step::Speculate, Step::Watchdog];

impl Step {
    pub fn describe(self) -> &'static str {
        match self {
            Step::Between => "waiting between rounds",
            Step::Settle => "settling finished after-landing runs and jobs",
            Step::Jobs => "starting the sheriff, recovery and outbox pull jobs",
            Step::Prepare => "fetching main",
            Step::Build => "building the candidate",
            Step::Check => "checking the candidate",
            Step::Culprit => "searching for the culprit lane",
            Step::Push => "pushing the candidate",
            Step::Finish => "recording the landed lanes",
            Step::Speculate => "landing the stacked candidates",
            Step::Watchdog => "running the watchdog",
        }
    }

    fn expected(self) -> bool {
        self == Step::Between
    }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

#[derive(Default)]
pub struct Heartbeat {
    step: AtomicU8,
    since: AtomicU64,
}

impl Heartbeat {
    pub fn enter(&self, step: Step) {
        self.since.store(now_millis(), Ordering::Relaxed);
        self.step.store(STEPS.iter().position(|known| *known == step).and_then(|index| u8::try_from(index).ok()).unwrap_or(0), Ordering::Relaxed);
    }

    pub fn current(&self) -> (Step, Duration) {
        let step = STEPS.get(usize::from(self.step.load(Ordering::Relaxed))).copied().unwrap_or(Step::Between);
        (step, Duration::from_millis(now_millis().saturating_sub(self.since.load(Ordering::Relaxed))))
    }
}

pub fn stuck(step: Step, held: Duration, reported: Option<Duration>) -> bool {
    !step.expected() && held >= STUCK && reported.is_none_or(|last| held >= last + REPEAT)
}

fn watch(lander: &Lander, done: &AtomicBool) {
    let mut reported: Option<(u64, Duration)> = None;
    while !done.load(Ordering::Relaxed) {
        std::thread::sleep(LOOK);
        let (step, held) = lander.heartbeat.current();
        let since = lander.heartbeat.since.load(Ordering::Relaxed);
        let last = reported.filter(|(at, _)| *at == since).map(|(_, held)| held);
        if stuck(step, held, last) {
            println!("north land watchdog: the serving loop of {} has been {} for {} s", lander.repo.display(), step.describe(), held.as_secs());
            reported = Some((since, held));
        }
    }
}

struct Stop<'a>(&'a AtomicBool);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub fn watched<T>(lander: &Lander, serve: impl FnOnce() -> T) -> T {
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| watch(lander, &done));
        let _stop = Stop(&done);
        serve()
    })
}

#[cfg(test)]
#[path = "heartbeat_tests.rs"]
mod tests;
