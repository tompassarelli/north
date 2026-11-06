//! What a measured value says about a goal: green, red within its debt ceiling, or red for good.
use super::registry::{Class, Goal};

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Green(f64),
    RedContract(f64),
    RedDebt(f64),
    RedOverCeiling(f64),
    Unmeasured(String),
}

pub fn judge(goal: &Goal, reading: Result<f64, String>) -> Verdict {
    let value = match reading {
        Ok(value) => value,
        Err(why) => return Verdict::Unmeasured(why),
    };
    match (goal.bound.holds(value), goal.class, &goal.debt) {
        (true, ..) => Verdict::Green(value),
        (false, Class::Contract, _) => Verdict::RedContract(value),
        (false, Class::Debt, Some(debt)) if !beyond(goal, debt.ceiling, value) => Verdict::RedDebt(value),
        (false, Class::Debt, _) => Verdict::RedOverCeiling(value),
        (false, Class::Ready, _) => Verdict::Unmeasured("a ready bar is read by north scoreboard --ready".into()),
    }
}

fn beyond(goal: &Goal, ceiling: f64, value: f64) -> bool {
    match goal.bound {
        super::registry::Bound::Max(_) => value > ceiling,
        super::registry::Bound::Min(_) => value < ceiling,
    }
}

impl Verdict {
    pub fn green(&self) -> bool {
        matches!(self, Self::Green(_))
    }

    pub fn failing(&self) -> bool {
        !matches!(self, Self::Green(_) | Self::RedDebt(_))
    }

    pub fn word(&self) -> &'static str {
        match self {
            Self::Green(_) => "green",
            Self::RedContract(_) => "RED",
            Self::RedDebt(_) => "red, debt within ceiling",
            Self::RedOverCeiling(_) => "RED, debt over ceiling",
            Self::Unmeasured(_) => "UNMEASURED",
        }
    }
}
