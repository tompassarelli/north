use crate::error::Result;
use crate::plan::{Trial, Unit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub rate_limited: bool,
    pub effort: Option<String>,
}

pub trait Attempts: Sync {
    fn attempt(&self, trial: &Trial, pass: u32, attempt: u32) -> Result<Attempt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitOutcome {
    pub passes: u32,
    pub valid: bool,
    pub efforts: Vec<Option<String>>,
}

pub fn run_unit(driver: &dyn Attempts, unit: &Unit, max_reruns: u32) -> Result<UnitOutcome> {
    let last_pass = max_reruns + 1;
    for pass in 1..=last_pass {
        let mut efforts = Vec::with_capacity(unit.trials.len());
        let mut trials_valid = true;
        for trial in &unit.trials {
            let (attempt, valid) = run_trial(driver, trial, pass, max_reruns)?;
            trials_valid &= valid;
            efforts.push(attempt.effort);
        }
        let differ = efforts.windows(2).any(
            |pair| matches!((&pair[0], &pair[1]), (Some(first), Some(second)) if first != second),
        );
        if !differ || pass == last_pass {
            return Ok(UnitOutcome {
                passes: pass,
                valid: !differ && trials_valid && efforts.iter().all(Option::is_some),
                efforts,
            });
        }
    }
    unreachable!("the last pass always returns")
}

fn run_trial(
    driver: &dyn Attempts,
    trial: &Trial,
    pass: u32,
    max_reruns: u32,
) -> Result<(Attempt, bool)> {
    let mut attempt_number = 1;
    loop {
        let attempt = driver.attempt(trial, pass, attempt_number)?;
        if !attempt.rate_limited {
            return Ok((attempt, true));
        }
        if attempt_number > max_reruns {
            return Ok((attempt, false));
        }
        attempt_number += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::manifest;
    use crate::plan::Plan;

    struct Scripted {
        script: Mutex<Vec<Attempt>>,
        calls: Mutex<Vec<(String, u32, u32)>>,
    }

    impl Attempts for Scripted {
        fn attempt(&self, trial: &Trial, pass: u32, attempt: u32) -> Result<Attempt> {
            self.calls
                .lock()
                .map_err(|_| crate::error::RunError::Failed("poisoned".to_owned()))?
                .push((trial.id.clone(), pass, attempt));
            let mut script = self
                .script
                .lock()
                .map_err(|_| crate::error::RunError::Failed("poisoned".to_owned()))?;
            Ok(script.remove(0))
        }
    }

    fn effort(level: &str) -> Option<String> {
        Some(level.to_owned())
    }

    fn pair_unit() -> Unit {
        let tasks = manifest::all().expect("table parses");
        Plan::full(1, &tasks).units.swap_remove(0)
    }

    #[test]
    fn a_rate_limited_trial_is_invalid_and_rerun_until_it_runs_clean() {
        let unit = pair_unit();
        let driver = Scripted {
            script: Mutex::new(vec![
                Attempt {
                    rate_limited: true,
                    effort: effort("medium"),
                },
                Attempt {
                    rate_limited: false,
                    effort: effort("medium"),
                },
                Attempt {
                    rate_limited: false,
                    effort: effort("medium"),
                },
            ]),
            calls: Mutex::new(Vec::new()),
        };
        let outcome = run_unit(&driver, &unit, 2).expect("unit runs");
        assert!(outcome.valid);
        assert_eq!(outcome.passes, 1);
        let calls = driver.calls.lock().expect("calls").clone();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].2, 1);
        assert_eq!(calls[1].2, 2);
    }

    #[test]
    fn a_pair_at_different_efforts_is_rerun_as_a_whole() {
        let unit = pair_unit();
        let driver = Scripted {
            script: Mutex::new(vec![
                Attempt {
                    rate_limited: false,
                    effort: effort("medium"),
                },
                Attempt {
                    rate_limited: false,
                    effort: effort("high"),
                },
                Attempt {
                    rate_limited: false,
                    effort: effort("medium"),
                },
                Attempt {
                    rate_limited: false,
                    effort: effort("medium"),
                },
            ]),
            calls: Mutex::new(Vec::new()),
        };
        let outcome = run_unit(&driver, &unit, 1).expect("unit runs");
        assert!(outcome.valid);
        assert_eq!(outcome.passes, 2);
    }
}
