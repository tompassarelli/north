//! How often each escalation arm's runs succeeded, with a Wilson 95% interval.
use serde_json::Value;

pub(super) const QUERY: &str = "SELECT agent, arm, follows, outcome, expects_landing, landed, item FROM runs WHERE role = 'worker'";
const ARMS: [&str; 3] = ["escalate", "cheap-fix", "split"];
const ARM_MIN: usize = 15;
const Z95: f64 = 1.959964;

pub(crate) struct Run {
    agent: String,
    arm: String,
    follows: Option<String>,
    outcome: String,
    expects_landing: bool,
    landed: Option<i64>,
    item: Option<String>,
}

pub(super) fn read(rows: &[Value]) -> Vec<Run> {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    rows.iter()
        .map(|row| Run {
            agent: text(&row["agent"]).unwrap_or_default(),
            arm: text(&row["arm"]).unwrap_or_default().to_lowercase(),
            follows: text(&row["follows"]).filter(|follows| !follows.is_empty()),
            outcome: text(&row["outcome"]).unwrap_or_default(),
            expects_landing: row["expects_landing"].as_i64().is_some_and(|expects| expects != 0),
            landed: row["landed"].as_i64(),
            item: text(&row["item"]).filter(|item| !item.is_empty()),
        })
        .collect()
}

fn unjudged(run: &Run) -> bool {
    matches!(run.outcome.as_str(), "running" | "idle") || (run.outcome == "unclear" && run.item.is_none())
}

fn succeeded(run: &Run, followed: bool) -> bool {
    run.outcome == "done" && !followed && !(run.expects_landing && run.landed == Some(0))
}

pub(crate) fn wilson(conformance: f64, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let n = n as f64;
    let p = (conformance * n).round() / n;
    let centre = (p + Z95 * Z95 / (2.0 * n)) / (1.0 + Z95 * Z95 / n);
    let half = Z95 * (p * (1.0 - p) / n + Z95 * Z95 / (4.0 * n * n)).sqrt() / (1.0 + Z95 * Z95 / n);
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

pub(super) fn lines(runs: &[Run]) -> Vec<String> {
    let judged: Vec<&Run> = runs.iter().filter(|run| !unjudged(run)).collect();
    let splits: Vec<&Run> = judged.iter().copied().filter(|run| run.outcome == "split").collect();
    let is_split = |name: &str| splits.iter().any(|split| split.agent == name);
    let followed = |agent: &str| judged.iter().any(|run| run.follows.as_deref() == Some(agent));
    let mut tally = [(0usize, 0usize); 3];
    let mut count = |arm: &str, won: bool| {
        if let Some(slot) = ARMS.iter().position(|known| *known == arm) {
            tally[slot].1 += 1;
            tally[slot].0 += usize::from(won);
        }
    };
    for run in judged.iter().filter(|run| !is_split(&run.agent) && !run.follows.as_deref().is_some_and(is_split)) {
        count(&run.arm, succeeded(run, followed(&run.agent)));
    }
    for split in &splits {
        let parts: Vec<&&Run> = judged.iter().filter(|run| run.follows.as_deref() == Some(split.agent.as_str())).collect();
        if !parts.is_empty() {
            count(&split.arm, parts.iter().all(|part| succeeded(part, followed(&part.agent))));
        }
    }
    let cells: Vec<String> = ARMS
        .iter()
        .zip(tally)
        .map(|(arm, (won, n))| {
            let (low, high) = wilson(if n == 0 { 0.0 } else { won as f64 / n as f64 }, n);
            format!("{arm} {won}/{n} [{:.0}%-{:.0}%]", low * 100.0, high * 100.0)
        })
        .collect();
    let thin = tally.iter().any(|(_, n)| *n < ARM_MIN);
    vec![format!("escalation arms: {}{}", cells.join(", "), if thin { format!(" (needs {ARM_MIN} runs per arm)") } else { String::new() })]
}
