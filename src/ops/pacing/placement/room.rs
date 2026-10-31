//! The pace facts every account choice reads: what an account's plan holds, how much room its
//! windows leave, and the one ranking that orders accounts for launch, revive and dispatch.
use crate::pace::exhaustion;
use serde_json::Value;
use std::cmp::Ordering;
use toml_edit::{DocumentMut, Item};

pub(crate) const CATALOG: &str = include_str!("../../../../harness/model-catalog.toml");
pub(crate) const WEEK_H: f64 = 7.0 * 24.0;

/// What a plan holds of each window, in Max 5x units: Max 20x holds four times the five-hour room of Max 5x but only twice its week.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlanSize {
    pub five_hour: f64,
    pub week: f64,
}

/// Points of an account's own percent a launch takes from each window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Charge {
    pub five_hour: f64,
    pub week: f64,
}

/// Points an account has left, in its own percent, before the five-hour stop and before the week is exhausted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Room {
    pub five_hour: f64,
    pub week: f64,
    /// Hours until the week resets, when its unspent room is lost.
    pub week_reset_h: f64,
}

impl Room {
    const SOONEST_RESET_H: f64 = 1.0 / 60.0;

    /// Week room in plan units per hour until it resets: the capacity lost each hour it is not spent.
    pub(crate) fn expiring(&self, plan: PlanSize) -> f64 {
        self.week * plan.week / self.week_reset_h.max(Self::SOONEST_RESET_H)
    }

    pub(crate) fn less(self, charge: Charge) -> Self {
        Self { five_hour: self.five_hour - charge.five_hour, week: self.week - charge.week, ..self }
    }

    /// Weekly points of an account's own percent that one box reserves, which the week must hold beyond to staff it.
    pub(crate) fn box_reserve() -> f64 {
        crate::pace::limits::default("reservation_fallback_points")
    }

    /// Why the week's room, `reserve` points or fewer, keeps a box off the account, else None.
    pub(crate) fn week_short(&self, reserve: f64) -> Option<String> {
        (self.week <= reserve).then(|| format!("{:.0} points left in the week, no more than the {reserve:.0} a box reserves", self.week))
    }

    /// Whether `charge` more leaves both windows short of the point where the out-rule refuses.
    pub(crate) fn takes(&self, charge: Charge) -> bool {
        let after = self.less(charge);
        after.five_hour > 0.0 && after.week > 0.0
    }
}

/// An account's plan size per window in Max 5x units: the five-hour and weekly multipliers of its
/// `[accounts.PROVIDER] plans` entry in the model catalog, over those of `max-5x`.
pub(crate) fn plan(cfg: &DocumentMut, catalog: &DocumentMut, account: &str) -> Result<PlanSize, String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let named = cfg.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|policy| policy.get("plans")).and_then(|plans| plans.get(label)).and_then(Item::as_str);
    let plan = named.ok_or_else(|| format!("{account} has no [accounts.{provider}] plans entry, so dispatch sizes it as max-5x"))?;
    let plans = catalog.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|catalog| catalog.get("plans"));
    let multiples = |name: &str| {
        let entry = plans?.get(name)?;
        let multiple = |key: &str| entry.get(key).and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64))).filter(|times| *times > 0.0);
        Some((multiple("five_hour_multiplier")?, multiple("weekly_multiplier")?))
    };
    let unit = multiples("max-5x").ok_or_else(|| format!("the model catalog has no {provider} max-5x plan with five_hour_multiplier and weekly_multiplier, so dispatch sizes {account} as max-5x"))?;
    let (five_hour, week) = multiples(plan).ok_or_else(|| format!("{account} plan {plan} has no five_hour_multiplier and weekly_multiplier in the model catalog, so dispatch sizes it as max-5x"))?;
    Ok(PlanSize { five_hour: five_hour / unit.0, week: week / unit.1 })
}

/// The room a usage-gate control leaves at `at`: its tightest five-hour and week windows, an
/// unsampled window counting as unused.
pub(crate) fn room(control: &Value, at: f64) -> Room {
    let mut room = Room { five_hour: exhaustion::FIVE_HOUR_STOP, week: 100.0, week_reset_h: WEEK_H };
    for window in control["windows"].as_array().into_iter().flatten() {
        let (Some(name), Some(used)) = (window["window"].as_str(), window["used_pct"].as_f64()) else { continue };
        let Some(left) = exhaustion::headroom(name, used) else { continue };
        if name == "5h" {
            room.five_hour = room.five_hour.min(left);
        } else if left <= room.week {
            let reset = window["resets_at"].as_str().and_then(crate::pace::target::seconds).map(|resets| (resets - at) / 3600.0);
            room = Room { week: left, week_reset_h: reset.or_else(|| window["reset_h"].as_f64()).unwrap_or(WEEK_H), ..room };
        }
    }
    room
}

/// What orders accounts for a launch: a live spend target first, since Tom set it to drain that
/// account; then the week's room that expires soonest, in plan units per hour until its reset; then
/// five-hour room in plan units. `north fleet launch`, revive and dispatch all rank by this.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rank {
    spending: bool,
    expiring: f64,
    five_hour: f64,
    /// What one more point of week room would add to `expiring`.
    per_week_point: f64,
}

impl Rank {
    pub(crate) fn of(room: Room, plan: PlanSize, spending: bool) -> Self {
        Self { spending, expiring: room.expiring(plan), five_hour: room.five_hour * plan.five_hour, per_week_point: Room { week: 1.0, ..room }.expiring(plan) }
    }

    pub(crate) fn order(&self, other: &Self) -> Ordering {
        self.spending.cmp(&other.spending).then(self.expiring.total_cmp(&other.expiring)).then(self.five_hour.total_cmp(&other.five_hour))
    }

    /// Whether this account would rank with `best` given `points` more week room, so a launch that names
    /// it keeps the caller's choice instead of flipping on the next quota sample.
    pub(crate) fn ties(&self, best: &Self, points: f64) -> bool {
        self.spending == best.spending && self.expiring + points * self.per_week_point >= best.expiring
    }
}

/// The configuration and pace state accounts are ranked from.
pub(crate) struct Basis<'a> {
    cfg: &'a DocumentMut,
    catalog: DocumentMut,
    pace: Value,
    at: f64,
}

impl<'a> Basis<'a> {
    pub(crate) fn new(cfg: &'a DocumentMut, pace: Value, at: f64) -> Self {
        Self { cfg, catalog: CATALOG.parse().expect("harness/model-catalog.toml parses"), pace, at }
    }

    pub(crate) fn plan(&self, account: &str) -> Result<PlanSize, String> {
        plan(self.cfg, &self.catalog, account)
    }

    pub(crate) fn spending(&self, account: &str) -> bool {
        self.pace["accounts"][account]["target"]["by"].as_str().and_then(crate::pace::target::seconds).is_some_and(|by| by > self.at)
    }

    pub(crate) fn rank(&self, available: bool, account: &str, usage: &Value) -> Rank {
        if !available {
            return Rank::default();
        }
        Rank::of(room(usage, self.at), self.plan(account).unwrap_or(PlanSize { five_hour: 1.0, week: 1.0 }), self.spending(account))
    }
}
