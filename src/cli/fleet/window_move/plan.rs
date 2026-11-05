use crate::usage::outlook::{AccountOutlook, Runway};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn outlook(account: &str, week_used: f64, runway: Runway) -> AccountOutlook {
        AccountOutlook { account: account.to_owned(), week_used: Some(week_used), runway }
    }

    fn morning() -> Vec<AccountOutlook> {
        vec![
            outlook("a", 72.0, Runway::Empties { minutes: 712.0 }),
            outlook("b", 100.0, Runway::Out),
            outlook("c", 18.0, Runway::Empties { minutes: 1045.0 }),
            outlook("d", 99.0, Runway::Empties { minutes: 89.0 }),
        ]
    }

    #[test]
    fn an_outlook_under_45_minutes_moves_the_window_to_the_account_with_the_most_week_headroom() {
        let mut outlooks = morning();
        outlooks[3].runway = Runway::Empties { minutes: 44.0 };
        match decide("d", &outlooks, None) {
            Verdict::Move { to, .. } => assert_eq!(to, "c"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_outlook_of_45_minutes_or_more_stays() {
        let mut outlooks = morning();
        outlooks[3].runway = Runway::Empties { minutes: 45.0 };
        assert!(matches!(decide("d", &outlooks, None), Verdict::Stay { .. }));
        assert!(matches!(decide("d", &morning(), None), Verdict::Stay { .. }));
    }

    #[test]
    fn an_account_that_is_out_moves_and_never_picks_an_out_or_endangered_account() {
        let mut outlooks = morning();
        outlooks[3].runway = Runway::Out;
        outlooks[2].runway = Runway::Empties { minutes: 30.0 };
        match decide("d", &outlooks, None) {
            Verdict::Move { to, .. } => assert_eq!(to, "a"),
            other => panic!("{other:?}"),
        }
        outlooks[0].runway = Runway::Out;
        assert!(matches!(decide("d", &outlooks, None), Verdict::Stranded { .. }));
    }
}
