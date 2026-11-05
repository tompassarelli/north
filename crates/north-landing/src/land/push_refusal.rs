use std::time::Duration;

const TERMINAL: [&str; 5] = ["archived", "read-only", "403", "permission", "denied"];
const ALARM_AT: u32 = 3;

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
    pub fn refused(&mut self) -> Pause {
        self.strikes = self.strikes.saturating_add(1);
        Pause { wait: Duration::from_secs(60 << (self.strikes - 1).min(2)), alarm: self.strikes == ALARM_AT }
    }

    pub fn clear(&mut self) {
        self.strikes = 0;
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
        assert_eq!(classify("To origin\n ! [remote rejected] main -> main (pre-receive hook declined)\n"), Refusal::Retry("! [remote rejected] main -> main (pre-receive hook declined)".to_owned()));
        assert_eq!(classify(""), Refusal::Retry("git push failed without a message".to_owned()));
    }

    #[test]
    fn other_refusals_back_off_one_two_four_minutes_and_alarm_on_the_third() {
        let mut backoff = Backoff::default();
        let waits: Vec<Pause> = (0..4).map(|_| backoff.refused()).collect();
        let minutes = |minutes: u64| Duration::from_secs(60 * minutes);
        assert_eq!(waits, [Pause { wait: minutes(1), alarm: false }, Pause { wait: minutes(2), alarm: false }, Pause { wait: minutes(4), alarm: true }, Pause { wait: minutes(4), alarm: false }]);
        backoff.clear();
        assert_eq!(backoff.refused(), Pause { wait: minutes(1), alarm: false });
    }
}
