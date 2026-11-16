//! The phase a dispatch round is in and the seconds each phase took, which the round's limit names when it ends a round that outlived it and the host record carries.
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

struct Phases {
    current: &'static str,
    since: Option<Instant>,
    seconds: BTreeMap<&'static str, f64>,
}

static PHASES: Mutex<Phases> = Mutex::new(Phases { current: "start", since: None, seconds: BTreeMap::new() });

pub fn enter(phase: &'static str) {
    if let Ok(mut phases) = PHASES.lock() {
        let now = Instant::now();
        if let Some(since) = phases.since {
            let current = phases.current;
            *phases.seconds.entry(current).or_default() += now.duration_since(since).as_secs_f64();
        }
        phases.current = phase;
        phases.since = Some(now);
    }
}

pub fn current() -> &'static str {
    PHASES.lock().map_or("unknown", |phases| phases.current)
}

/// Runs `read` and adds its seconds under `name` without leaving the current phase, for reads that run beside each other.
pub fn timed<T>(name: &'static str, read: impl FnOnce() -> T) -> T {
    let began = Instant::now();
    let value = read();
    if let Ok(mut phases) = PHASES.lock() {
        *phases.seconds.entry(name).or_default() += began.elapsed().as_secs_f64();
    }
    value
}

/// The seconds of every phase so far, the current one up to now.
pub fn seconds() -> BTreeMap<String, f64> {
    let Ok(phases) = PHASES.lock() else { return BTreeMap::new() };
    let mut seconds: BTreeMap<String, f64> = phases.seconds.iter().map(|(name, seconds)| ((*name).to_owned(), *seconds)).collect();
    if let Some(since) = phases.since {
        *seconds.entry(phases.current.to_owned()).or_default() += since.elapsed().as_secs_f64();
    }
    seconds
}

/// One line of the seconds each phase took so far, longest first.
pub fn line() -> String {
    let mut seconds: Vec<(String, f64)> = seconds().into_iter().collect();
    seconds.sort_by(|left, right| right.1.total_cmp(&left.1));
    let phases: Vec<String> = seconds.iter().map(|(name, seconds)| format!("{name} {seconds:.1}")).collect();
    format!("north fleet dispatch: seconds per phase: {}", phases.join(", "))
}
