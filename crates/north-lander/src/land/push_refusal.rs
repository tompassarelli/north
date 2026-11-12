use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const TERMINAL: [&str; 5] = ["archived", "read-only", "403", "permission", "denied"];
const ALARM_AT: u32 = 3;
const LOCKED: &str = "database is locked";
const LOCKED_RETRY: Duration = Duration::from_millis(250);
const RETIRE_POLL: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Terminal(String),
    Retry(String),
}

fn terminal(line: &str) -> bool {
    let line = line.to_lowercase();
    TERMINAL.iter().any(|word| line.contains(word))
}

pub fn classify(stderr: &str) -> Refusal {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    match lines.iter().find(|line| terminal(line)) {
        Some(line) => Refusal::Terminal((*line).to_owned()),
        None => Refusal::Retry(lines.last().map_or_else(|| "git push failed without a message".to_owned(), |line| (*line).to_owned())),
    }
}

#[derive(Debug, Default)]
pub struct Backoff {
    strikes: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Pause {
    pub wait: Duration,
    pub alarm: bool,
}

impl Backoff {
    pub fn after_store_error(&mut self, error: &str) -> Pause {
        if error.contains(LOCKED) {
            return Pause { wait: LOCKED_RETRY, alarm: false };
        }
        self.after_push_refusal()
    }

    pub fn after_push_refusal(&mut self) -> Pause {
        self.strikes = self.strikes.saturating_add(1);
        Pause { wait: Duration::from_secs(60 << (self.strikes - 1).min(2)), alarm: self.strikes == ALARM_AT }
    }

    pub fn clear(&mut self) {
        self.strikes = 0;
    }

    pub fn clear_if(&mut self, healthy: bool) {
        if healthy {
            self.clear();
        }
    }
}

impl Pause {
    fn naps(&self) -> impl Iterator<Item = Duration> {
        let mut left = self.wait;
        std::iter::from_fn(move || {
            let nap = left.min(RETIRE_POLL);
            left -= nap;
            (!nap.is_zero()).then_some(nap)
        })
    }

    pub fn sit_out(&self, retire: &AtomicBool) {
        for nap in self.naps() {
            if retire.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(nap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archived_or_forbidden_refusal_is_terminal_and_names_the_remote_line() {
        let archived = "remote: This repository was archived so it is read-only.\nfatal: unable to access 'https://github.com/o/wake.git/': The requested URL returned error: 403\n";
        assert_eq!(classify(archived), Refusal::Terminal("remote: This repository was archived so it is read-only.".to_owned()));
        assert_eq!(classify("remote: Permission to o/r.git denied to lander[bot].\n"), Refusal::Terminal("remote: Permission to o/r.git denied to lander[bot].".to_owned()));
        assert_eq!(classify("remote: Permission to o/r.git denied to north-lander[bot].\n"), Refusal::Terminal("remote: Permission to o/r.git denied to north-lander[bot].".to_owned()));
        assert_eq!(classify("To origin\n ! [remote rejected] main -> main (pre-receive hook declined)\n"), Refusal::Retry("! [remote rejected] main -> main (pre-receive hook declined)".to_owned()));
        assert_eq!(classify(""), Refusal::Retry("git push failed without a message".to_owned()));
    }

    #[test]
    fn a_lander_serves_the_next_ticket_within_two_seconds_of_the_lifecycle_store_unlocking() {
        let mut backoff = Backoff::default();
        let locked = backoff.after_store_error("Incomplete North state: lifecycle store: database is locked");
        assert!(locked.naps().eq([LOCKED_RETRY]) && !locked.alarm, "a locked store held the lander {locked:?}, as the 60 s pause held the next lane 83 s on 2026-11-12");
        assert_eq!(Pause { wait: Duration::from_secs(5), alarm: false }.naps().collect::<Vec<_>>(), [RETIRE_POLL, RETIRE_POLL, Duration::from_secs(1)]);
        assert_eq!(backoff.after_push_refusal().wait, Duration::from_secs(60));
        assert_eq!(backoff.after_store_error("lifecycle store: database is locked").wait, LOCKED_RETRY);
        assert_eq!(backoff.after_push_refusal().wait, Duration::from_secs(120));
        let echoed = backoff.after_push_refusal();
        assert_eq!((echoed.wait, echoed.alarm), (Duration::from_secs(240), true), "a push refusal reading {LOCKED:?} took {echoed:?}, not the 240 s rung with its alarm");
        backoff.clear();
        assert_eq!(backoff.after_push_refusal().wait, Duration::from_secs(60));
    }
}
