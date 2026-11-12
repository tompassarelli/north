use serde_json::json;

use crate::error::Result;
use crate::plan::{Trial, Unit};
use crate::trial::{Attempt, Live};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitOutcome {
    pub passes: u32,
    pub valid: bool,
    pub efforts: Vec<Option<String>>,
}

pub fn run_unit(driver: &Live<'_>, unit: &Unit, max_reruns: u32) -> Result<UnitOutcome> {
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
    driver: &Live<'_>,
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

pub fn record(live: &Live<'_>, unit: &Unit, outcome: &UnitOutcome) -> Result<()> {
    let task = unit.task();
    live.record(&json!({
        "kind": "unit",
        "run": live.run_id,
        "unit": unit.index + 1,
        "phase": unit.trials[0].phase.label(),
        "task": task.name,
        "large": task.is_large(),
        "virtual_machine": task.is_virtual_machine(),
        "passes": outcome.passes,
        "valid": outcome.valid,
        "efforts": outcome.efforts,
    }))
}
