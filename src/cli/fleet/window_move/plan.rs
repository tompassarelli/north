use crate::usage::outlook::{AccountOutlook, Runway};

use super::session::Window;

pub(super) const TRIGGER_MINUTES: f64 = 45.0;

#[derive(Debug, PartialEq)]
pub(super) enum Verdict {
    Stay { reason: String },
    Move { to: String, reason: String },
    Stranded { reason: String },
}

pub(super) fn endangered(outlook: &AccountOutlook) -> bool {
    match outlook.runway {
        Runway::Out => true,
        Runway::Empties { minutes } => minutes < TRIGGER_MINUTES,
        Runway::Lasts | Runway::Unknown => false,
    }
}

fn headroom(outlook: &AccountOutlook) -> f64 {
    100.0 - outlook.week_used.unwrap_or(100.0)
}

fn fit_target(outlook: &AccountOutlook) -> bool {
    outlook.week_used.is_some_and(|used| used < 100.0) && !endangered(outlook)
}

pub(super) fn decide(current: &str, outlooks: &[AccountOutlook], requested: Option<&str>) -> Verdict {
    let Some(mine) = outlooks.iter().find(|outlook| outlook.account == current) else {
        return Verdict::Stranded { reason: format!("account {current} has no usage reading") };
    };
    if let Some(to) = requested {
        return match outlooks.iter().find(|outlook| outlook.account == to) {
            _ if to == current => Verdict::Stay { reason: format!("the window already runs on account {current}") },
            Some(target) if fit_target(target) => Verdict::Move { to: to.to_owned(), reason: format!("asked to move to account {to} with {:.0}% of its week left", headroom(target)) },
            Some(_) => Verdict::Stranded { reason: format!("account {to} is out or burning toward its limit") },
            None => Verdict::Stranded { reason: format!("account {to} has no usage reading") },
        };
    }
    if !endangered(mine) {
        return Verdict::Stay { reason: format!("account {current} lasts past {TRIGGER_MINUTES:.0} minutes") };
    }
    let best = outlooks.iter().filter(|outlook| outlook.account != current && fit_target(outlook)).max_by(|left, right| headroom(left).total_cmp(&headroom(right)).then_with(|| right.account.cmp(&left.account)));
    match best {
        Some(target) => Verdict::Move { to: target.account.clone(), reason: format!("account {current} runs out in under {TRIGGER_MINUTES:.0} minutes; account {} has the most week headroom at {:.0}%", target.account, headroom(target)) },
        None => Verdict::Stranded { reason: format!("account {current} runs out in under {TRIGGER_MINUTES:.0} minutes and no other account has week headroom") },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Step {
    Carry,
    Open,
    Rebind,
    Deliver,
    EndOld,
}

pub(super) const STEPS: [Step; 5] = [Step::Carry, Step::Open, Step::Rebind, Step::Deliver, Step::EndOld];

impl Step {
    pub(super) fn describe(self, window: &Window, to: &str) -> String {
        let (from, conversation) = (&window.account, &window.session.id);
        match self {
            Step::Carry => format!("carry conversation {conversation} into account {to}'s profile"),
            Step::Open => format!("open a new window resuming conversation {conversation} on account {to} and confirm it runs there"),
            Step::Rebind => format!("change the fleet record's account from {from} to {to} and bind the coordinator's name to the new window"),
            Step::Deliver => "deliver the coordinator's undelivered messages to the new window".to_owned(),
            Step::EndOld => format!("end the old window (process {}) on account {from} at the end of its turn", window.client),
        }
    }
}
