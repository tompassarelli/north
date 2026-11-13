//! Why placement does not start a launch: a saturated host may take it once its admission queue,
//! disk or load eases, a spend refusal once a window resets (docs/rationale/spend-rule.md), and a
//! landing line over the wait bar once a build host's line is under it, while anything else needs a different launch.
use crate::error::NorthError;
use north_strategy_types::placement as record;

#[derive(Debug, PartialEq)]
pub(super) enum Refusal {
    Saturated(String),
    Spend(String),
    Final(String),
    Line(String),
}

impl Refusal {
    /// Whether the launch queue holds a launch this refusal stops: a spend refusal until a window resets, a landing line over the
    /// bar until it eases, and a saturated host for a harness launch, which has no wait of its own.
    pub(super) fn queues(&self, provider: &str) -> bool {
        matches!(self, Self::Spend(_) | Self::Line(_)) || (provider != "claude" && matches!(self, Self::Saturated(_)))
    }

    /// The hold a harness launch meets while every build host's landing line is over the bar: a worker waits, unless the coordinator
    /// granted it urgent priority, which passes the hold and says so.
    pub(super) fn line_hold(worker: bool, urgent: bool) -> Option<Self> {
        let why = worker.then(crate::pacing::placement::line_held_now).flatten()?;
        if !urgent {
            return Some(Self::Line(why));
        }
        eprintln!("fleet launch: urgent priority passes the hold: {why}");
        None
    }

    pub(super) fn kind(&self) -> record::Refusal {
        match self {
            Self::Saturated(_) => record::Refusal::Saturated,
            Self::Spend(_) => record::Refusal::Spend,
            Self::Final(_) => record::Refusal::Final,
            Self::Line(_) => record::Refusal::Line,
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Saturated(why) | Self::Spend(why) | Self::Final(why) | Self::Line(why) => formatter.write_str(why),
        }
    }
}

impl From<Refusal> for NorthError {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Spend(why) | Refusal::Line(why) => Self::Saturated(why),
            Refusal::Saturated(_) | Refusal::Final(_) => Self::Refused(refusal.to_string()),
        }
    }
}
