//! Rule 3: a burst of spend on one account stops new workers there until the burst falls out of its
//! window. The fleet burst is the rise of the account's five-hour window; a session burst is one
//! session's settled spend priced in the account's five-hour points.
use serde::{Deserialize, Serialize};

use super::Rules;

/// More than `points` of a five-hour window within `minutes`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub points: f64,
    pub minutes: f64,
}

/// One five-hour reading of an account, in unix seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: f64,
    pub used_pct: f64,
    pub resets_at: Option<f64>,
}

/// One session's settled spend: its cumulative dollars at each tick that read it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Spender {
    pub session: String,
    pub account: String,
    pub spent: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Trip {
    pub account: String,
    pub line: String,
    pub at: f64,
    #[serde(default)]
    pub priced: Option<f64>,
}

/// How far the five-hour window rose in the last `minutes`: the latest reading less the earliest
/// one inside the span that shares its reset.
pub fn rise(samples: &[Sample], now: f64, minutes: f64) -> f64 {
    let Some(latest) = samples.iter().filter(|sample| sample.at <= now).max_by(|a, b| a.at.total_cmp(&b.at)) else { return 0.0 };
    let since = now - minutes * 60.0;
    samples
        .iter()
        .filter(|sample| sample.at >= since && sample.at <= now && sample.resets_at == latest.resets_at)
        .min_by(|a, b| a.at.total_cmp(&b.at))
        .map_or(0.0, |first| (latest.used_pct - first.used_pct).max(0.0))
}

/// Dollars a session settled in the last `minutes`: its latest total less its total at the span's
/// start, or its first total inside the span when it started later.
pub fn spent(totals: &[(f64, f64)], now: f64, minutes: f64) -> f64 {
    let since = now - minutes * 60.0;
    let latest = totals.iter().filter(|(at, _)| *at <= now).max_by(|a, b| a.0.total_cmp(&b.0));
    let before = totals.iter().filter(|(at, _)| *at <= since).max_by(|a, b| a.0.total_cmp(&b.0));
    let first = totals.iter().filter(|(at, _)| *at > since && *at <= now).min_by(|a, b| a.0.total_cmp(&b.0));
    match (latest, before.or(first)) {
        (Some(latest), Some(start)) => (latest.1 - start.1).max(0.0),
        _ => 0.0,
    }
}

/// The trip on `account`, if the fleet's or one session's spend crossed its burst; `points_per_usd`
/// prices a session's dollars in the account's five-hour points.
pub fn trip(rules: &Rules, account: &str, samples: &[Sample], spenders: &[Spender], points_per_usd: f64, now: f64) -> Option<Trip> {
    let own: Vec<&Spender> = spenders.iter().filter(|spender| spender.account == account).collect();
    let points = |spender: &Spender, burst: Burst| spent(&spender.spent, now, burst.minutes) * points_per_usd;
    let rose = rise(samples, now, rules.fleet_trip.minutes);
    let fleet = (rose > rules.fleet_trip.points).then(|| format!("{account} five-hour window rose {rose:.0} points in {:.0} min, over the {:.0}-point tripwire", rules.fleet_trip.minutes, rules.fleet_trip.points));
    let session = own.iter().map(|spender| (spender.session.as_str(), points(spender, rules.session_trip))).filter(|(_, spent)| *spent > rules.session_trip.points).max_by(|a, b| a.1.total_cmp(&b.1));
    let session = session.map(|(name, spent)| format!("{name} spent {spent:.0} points of {account}'s five-hour window in {:.0} min, over the {:.0}-point session tripwire at {points_per_usd:.3} points per dollar", rules.session_trip.minutes, rules.session_trip.points));
    let priced = fleet.is_none().then_some(points_per_usd);
    let why = fleet.or(session)?;
    let mut top: Vec<(&str, f64)> = own.iter().map(|spender| (spender.session.as_str(), points(spender, rules.fleet_trip))).filter(|(_, spent)| *spent > 0.0).collect();
    top.sort_by(|a, b| b.1.total_cmp(&a.1));
    let named: Vec<String> = top.iter().take(3).map(|(name, spent)| format!("{name} {spent:.0}")).collect();
    let line = if named.is_empty() { why } else { format!("{why}; top spenders in points: {}", named.join(", ")) };
    Some(Trip { account: account.to_owned(), line, at: now, priced })
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Spenders {
    pub sessions: Vec<Spender>,
}

impl Spenders {
    pub fn record(&self, read: &[(String, String, f64)], now: f64, span: f64) -> Self {
        let since = now - span;
        let sessions = read
            .iter()
            .map(|(session, account, usd)| {
                let earlier = self.sessions.iter().find(|spender| spender.session == *session && spender.account == *account).map_or(&[][..], |spender| spender.spent.as_slice());
                let start = earlier.iter().filter(|(at, _)| *at <= since).map(|(at, _)| *at).fold(f64::NEG_INFINITY, f64::max);
                let mut spent: Vec<(f64, f64)> = earlier.iter().copied().filter(|(at, _)| *at >= start && *at < now).collect();
                spent.push((now, *usd));
                Spender { session: session.clone(), account: account.clone(), spent }
            })
            .collect();
        Self { sessions }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Judged {
    pub current: Vec<Trip>,
    pub new: Vec<Trip>,
}

/// The trips in force: every confirmed trip, and each earlier trip within `span` that this tick did not confirm, unless
/// a session trip priced at another rate than `price` gives its account now, so a recalibration lifts it at once.
pub fn judge(previous: &[Trip], confirmed: Vec<Trip>, now: f64, span: f64, price: impl Fn(&str) -> f64) -> Judged {
    let live: Vec<&Trip> = previous.iter().filter(|trip| now - trip.at <= span).collect();
    let new = confirmed.iter().filter(|trip| !live.iter().any(|earlier| earlier.account == trip.account)).cloned().collect();
    let repriced = |earlier: &Trip| earlier.priced.is_some_and(|priced| (priced - price(&earlier.account)).abs() > 1e-9);
    let kept: Vec<Trip> = live.into_iter().filter(|earlier| !confirmed.iter().any(|trip| trip.account == earlier.account) && !repriced(earlier)).cloned().collect();
    let mut current = confirmed;
    current.extend(kept);
    Judged { current, new }
}

pub const SPENDERS: &str = "spend/spenders.json";
pub const TRIP_LOG: &str = "spend/trips.jsonl";
