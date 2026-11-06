//! The pure judge: given an entry and its recorded readings, keep, revert, decide or re-measure.
use super::registry::{Day, Experiment, Metric, Window};

/// One metric over one window: its value, and how many samples stand behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub value: Option<f64>,
    pub n: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recommend {
    Keep,
    Revert,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Keep,
    Revert,
    Decide(Recommend),
    ReMeasure(String),
    /// The policy loop's own experiment, inside the bars on too few samples: judged again on a later review.
    Extend { after: Window, review: Day },
    /// The policy loop's own experiment, inside the bars at `min_n`: the loop keeps the cheaper arm.
    Settle(Recommend),
}

impl Verdict {
    /// Whether the verdict leaves work on its owner issue, which reopens it when closed.
    pub fn leaves_work(&self) -> bool {
        !matches!(self, Self::Keep | Self::Extend { .. } | Self::Settle(Recommend::Keep))
    }

    pub fn word(&self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Revert => "revert",
            Self::Decide(_) => "decide",
            Self::ReMeasure(_) => "re-measure",
            Self::Extend { .. } => "extend",
            Self::Settle(_) => "settle",
        }
    }
}

/// A verdict with the numbers it rests on: the baseline, the after value and the move toward the
/// predicted direction.
#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    pub verdict: Verdict,
    pub baseline: Option<f64>,
    pub after: Option<f64>,
    pub moved: Option<f64>,
}

/// Entries the policy climber registered, whose inconclusive verdicts it decides itself (docs/rationale/policy-climber.md).
pub const LOOP_PREFIX: &str = "climb-";

pub fn due(entry: &Experiment, today: Day) -> bool {
    today >= entry.review
}

fn usable(entry: &Experiment, window: Window, reading: Option<Reading>, side: &str) -> Result<f64, String> {
    let name = entry.metric.name();
    match reading {
        None => Err(format!("{name} has no recorded reading for the {side} window {}", window.text())),
        Some(Reading { value: None, n }) => Err(format!("{name} has no value for the {side} window {} (n={n})", window.text())),
        Some(Reading { n, .. }) if n < entry.min_n => Err(format!("{name} rests on n={n} in the {side} window {}, under the entry's min_n {}", window.text(), entry.min_n)),
        Some(Reading { value: Some(value), .. }) => Ok(value),
    }
}

/// Judges an entry from its readings. A missing or thin reading re-measures; it never guesses.
/// Without a before window the baseline is zero, so a count entry is judged on its after value.
/// The policy loop decides its own entries with no gate: a thin reading extends the after window and
/// the review by half the window, and a verdict between the bars settles on the recommended arm.
pub fn judge(entry: &Experiment, before: Option<Reading>, after: Option<Reading>) -> Judged {
    let judged = judged(entry, before, after);
    if !entry.id.starts_with(LOOP_PREFIX) {
        return judged;
    }
    let thin = |reading: Option<Reading>| reading.is_some_and(|reading| reading.value.is_some() && reading.n < entry.min_n);
    match judged.verdict {
        Verdict::Decide(recommend) => Judged { verdict: Verdict::Settle(recommend), ..judged },
        Verdict::ReMeasure(_) if thin(after) || entry.before.is_some() && thin(before) => {
            let more = ((entry.after.end.0 - entry.after.start.0) / 2).max(1);
            Judged { verdict: Verdict::Extend { after: Window { end: Day(entry.after.end.0 + more), ..entry.after }, review: Day(entry.review.0 + more) }, ..judged }
        }
        _ => judged,
    }
}

fn judged(entry: &Experiment, before: Option<Reading>, after: Option<Reading>) -> Judged {
    let remeasure = |why: String| Judged { verdict: Verdict::ReMeasure(why), baseline: None, after: None, moved: None };
    if let Metric::Unread(measure) = &entry.metric {
        return remeasure(format!("no north flow outcome metric reads this rule; it is judged by {measure}"));
    }
    let baseline = match entry.before {
        Some(window) => match usable(entry, window, before, "before") {
            Ok(value) => value,
            Err(why) => return remeasure(why),
        },
        None => 0.0,
    };
    let value = match usable(entry, entry.after, after, "after") {
        Ok(value) => value,
        Err(why) => return remeasure(why),
    };
    let moved = entry.direction.sign() * (value - baseline);
    let verdict = if moved > 0.0 && moved >= entry.bar {
        Verdict::Keep
    } else if moved <= -entry.bar {
        Verdict::Revert
    } else if moved > 0.0 {
        Verdict::Decide(Recommend::Keep)
    } else {
        Verdict::Decide(Recommend::Revert)
    };
    Judged { verdict, baseline: Some(baseline), after: Some(value), moved: Some(moved) }
}
