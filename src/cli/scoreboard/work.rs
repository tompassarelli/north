use super::{Better, Figure, Measure, Spent};
use crate::ledger::outcome::Week;

const SOURCE: &str = "worker-ledger run cost API-equivalent and Tom's conversation minutes over the window, divided by the window's landings that the rework judge cleared after 48 h (north ledger outcome)";

pub fn per_verified(read: &[Week]) -> [Measure; 2] {
    let figure = |kind: Spent| {
        std::array::from_fn(|index| {
            let (spent, verified) = read.get(index).map_or((0.0, 0), |week| {
                let total = &week.total;
                (if kind == Spent::Dollars { total.usd } else { week.attention.minutes }, total.landed.saturating_sub(total.reworked + total.unjudged + total.maturing))
            });
            Figure::Per { spent, kind, verified }
        })
    };
    [
        Measure { name: "cost per verified landing".into(), better: Better::Lower, source: SOURCE, weeks: figure(Spent::Dollars) },
        Measure { name: "Tom minutes per verified landing".into(), better: Better::Lower, source: SOURCE, weeks: figure(Spent::Minutes) },
    ]
}
