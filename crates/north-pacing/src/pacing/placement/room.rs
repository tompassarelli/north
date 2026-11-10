//! The order facts every account choice reads: what an account's plan holds, how much room its
//! windows leave before the provider's limit, and the one ranking that orders accounts for launch,
//! revive and dispatch. Whether an account may take a worker at all is the spend rule's call.
use serde_json::Value;
use std::cmp::Ordering;
use toml_edit::{DocumentMut, Item};

use crate::usage::decide::world::ModelFamily;
use crate::usage::meter;

pub const WEEK_H: f64 = 7.0 * 24.0;

/// What a plan holds of each window, in Max 5x units: Ultra 10x holds four times the five-hour room of Max 5x but only twice its week.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanSize {
    pub five_hour: f64,
    pub week: f64,
}

/// Points of an account's own percent a launch takes from each window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Charge {
    pub five_hour: f64,
    pub week: f64,
}

/// Points an account has left, in its own percent, before the provider's five-hour and week limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub five_hour: f64,
    pub week: f64,
    /// Hours until the week resets, when its unspent room is lost.
    pub week_reset_h: f64,
}

impl Room {
    pub const SOONEST_RESET_H: f64 = 1.0 / 60.0;

    /// Week room in plan units per hour until it resets: the capacity lost each hour it is not spent.
    pub fn expiring(&self, plan: PlanSize) -> f64 {
        self.week * plan.week / self.week_reset_h.max(Self::SOONEST_RESET_H)
    }

    pub fn less(self, charge: Charge) -> Self {
        Self { five_hour: self.five_hour - charge.five_hour, week: self.week - charge.week, ..self }
    }
}

/// An account's plan size per window in Max 5x units: the five-hour and weekly multipliers of its
/// `[accounts.PROVIDER] plans` entry in the model catalog, over those of `max-5x`.
pub fn plan(cfg: &DocumentMut, catalog: &DocumentMut, account: &str) -> Result<PlanSize, String> {
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

/// The room an account's sampled windows leave at `at`: its tightest five-hour and week windows, an
/// unsampled window or one whose reset has passed counting as unused.
pub fn room(windows: &Value, at: f64) -> Room {
    let mut room = Room { five_hour: 100.0, week: 100.0, week_reset_h: WEEK_H };
    for window in windows.as_array().into_iter().flatten() {
        let (Some(name), Some(used)) = (window["window"].as_str(), window["used_pct"].as_f64()) else { continue };
        if !matches!(name, "5h" | "week" | "weekly_all") {
            continue;
        }
        let resets = window["resets_at"].as_str().and_then(crate::foundation::clock::rfc3339_seconds);
        let left = if resets.is_some_and(|resets| resets <= at) { 100.0 } else { (100.0 - used).max(0.0) };
        if name == "5h" {
            room.five_hour = room.five_hour.min(left);
        } else if left <= room.week {
            let reset = resets.map(|resets| (resets - at) / 3600.0);
            room = Room { week: left, week_reset_h: reset.or_else(|| window["reset_h"].as_f64()).unwrap_or(WEEK_H), ..room };
        }
    }
    room
}

/// The room a model family leaves: its shared week tightened by the family's own scoped weekly meter
/// (`weekly_scoped/Fable`), or the shared week alone when the provider reported no scoped row.
pub fn room_for(windows: &Value, at: f64, family: ModelFamily) -> Room {
    let mut room = room(windows, at);
    let scoped = meter::scoped(family);
    for window in windows.as_array().into_iter().flatten() {
        if window["window"].as_str() != Some(scoped.as_str()) {
            continue;
        }
        let Some(used) = window["used_pct"].as_f64() else { continue };
        let resets = window["resets_at"].as_str().and_then(crate::foundation::clock::rfc3339_seconds);
        let left = if resets.is_some_and(|resets| resets <= at) { 100.0 } else { (100.0 - used).max(0.0) };
        if left <= room.week {
            let reset = resets.map(|resets| (resets - at) / 3600.0);
            room = Room { week: left, week_reset_h: reset.or_else(|| window["reset_h"].as_f64()).unwrap_or(WEEK_H), ..room };
        }
    }
    room
}

/// What orders accounts and API grants for a launch, earliest deadline first: a source the fleet cannot
/// drain before its deadline, then the one whose capacity is lost soonest (a plan's weekly reset, a
/// grant's expiry), then the week's room that expires soonest in plan units per hour until its reset,
/// then five-hour room in plan units. `north fleet launch`, revive and dispatch all rank by this, and it never refuses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rank {
    urgent: bool,
    deadline_h: Option<f64>,
    expiring: f64,
    five_hour: f64,
    /// Set only when an account is ranked by the room its own family's meter leaves, as the coordinator is.
    headroom: Option<f64>,
}

impl Rank {
    pub fn of(room: Room, plan: PlanSize) -> Self {
        let deadline_h = (room.week > 0.0).then(|| room.week_reset_h.max(Room::SOONEST_RESET_H));
        Self { urgent: false, deadline_h, expiring: room.expiring(plan), five_hour: room.five_hour * plan.five_hour, headroom: None }
    }

    /// Order accounts by the room their own family's meter leaves, so the coordinator starts where its
    /// Fable week has the most headroom rather than where the shared week expires soonest.
    pub fn headroom(room: f64) -> Self {
        Self { urgent: false, deadline_h: None, expiring: 0.0, five_hour: 0.0, headroom: Some(room) }
    }

    /// An API grant with credit left: its deadline is its expiry, and it is urgent when the measured burn would not spend it by then.
    pub fn grant(deadline_h: f64, urgent: bool) -> Self {
        Self { urgent, deadline_h: Some(deadline_h.max(Room::SOONEST_RESET_H)), expiring: 0.0, five_hour: 0.0, headroom: None }
    }

    pub fn order(&self, other: &Self) -> Ordering {
        let soonest = |rank: &Self| rank.deadline_h.map_or(0.0, |hours| 1.0 / hours);
        self.urgent
            .cmp(&other.urgent)
            .then(match (self.headroom, other.headroom) {
                (Some(left), Some(right)) => left.total_cmp(&right),
                _ => Ordering::Equal,
            })
            .then(soonest(self).total_cmp(&soonest(other)))
            .then(self.expiring.total_cmp(&other.expiring))
            .then(self.five_hour.total_cmp(&other.five_hour))
    }

    /// Why the source ranks where it does, as a placement records it.
    pub fn reason(&self) -> String {
        if let Some(headroom) = self.headroom {
            return format!("{headroom:.0}% headroom");
        }
        match (self.urgent, self.deadline_h) {
            (true, Some(hours)) => format!("cannot drain before its deadline in {hours:.0}h"),
            (false, Some(hours)) => format!("deadline in {hours:.0}h"),
            (_, None) => "no room before its deadline".to_owned(),
        }
    }
}

/// The configuration accounts are ranked from.
pub struct Basis<'a> {
    cfg: &'a DocumentMut,
    catalog: &'static DocumentMut,
    at: f64,
}

impl<'a> Basis<'a> {
    pub fn new(cfg: &'a DocumentMut, at: f64) -> Self {
        Self { cfg, catalog: crate::platform::harness::document(crate::platform::harness::MODEL_CATALOG), at }
    }

    pub fn plan(&self, account: &str) -> Result<PlanSize, String> {
        plan(self.cfg, self.catalog, account)
    }

    /// Why `account` takes no worker at all: `[accounts.PROVIDER] reserve` names it (a plan account by label, an API organization as `api:ORG`) for the privacy judge, lander and canaries.
    pub fn operational(&self, account: &str) -> Option<String> {
        crate::spend::operational_reserve(self.cfg).iter().any(|name| name == account).then(|| "reserve: operational callers only (privacy judge, lander, canaries)".to_owned())
    }

    /// Why `account` takes no launch yet: its `[accounts.PROVIDER] held_until_spent` entry names another account
    /// whose week in `windows` (the usage gate's windows by account) still has room.
    pub fn held(&self, account: &str, windows: &Value) -> Option<String> {
        let (provider, label) = account.split_once(':')?;
        let first = self.cfg.get("accounts")?.get(provider)?.get("held_until_spent")?.get(label)?.as_str()?;
        let first = format!("{provider}:{first}");
        let week = room(&windows[&first], self.at).week;
        (week > 0.0).then(|| format!("held until {first} is spent ({week:.0}% of its week left)"))
    }

    pub fn rank(&self, available: bool, account: &str, usage: &Value) -> Rank {
        if !available {
            return Rank::default();
        }
        Rank::of(room(usage, self.at), self.plan(account).unwrap_or(PlanSize { five_hour: 1.0, week: 1.0 }))
    }

    /// The rank of one model family's room: the shared week tightened by that family's scoped meter.
    pub fn rank_for(&self, family: ModelFamily, available: bool, account: &str, usage: &Value) -> Rank {
        if !available {
            return Rank::default();
        }
        Rank::of(room_for(usage, self.at, family), self.plan(account).unwrap_or(PlanSize { five_hour: 1.0, week: 1.0 }))
    }
}
