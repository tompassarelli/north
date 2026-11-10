//! What the runs of the window cost by model and effort, how much of each subscription's week that was, and what the
//! runs billed to an API organization's credit cost. A subscription's week in dollars is estimated from the runs it
//! ran since its week began and the share of the week its reading says is used, so a share is an estimate.
use std::collections::BTreeMap;

use super::Screen;
use crate::foundation::clock::Timestamp;

const API_ACCOUNT: &str = "claude-api";
const LOGIN_PREFIX: &str = "claude-";
const WEEK_SECONDS: i64 = 7 * 86_400;
/// Below this share of a week used, the spend since the week began is too small to size the week from.
const MIN_USED_PCT: f64 = 5.0;

#[derive(Clone, Debug)]
pub struct Class {
    pub mix: String,
    pub runs: usize,
    pub usd: f64,
    /// The share of each subscription's week this class used, by account letter: 0 where the week is sized and the class ran nothing on it, absent where the week cannot be sized.
    pub week_pct: BTreeMap<String, f64>,
    pub all_pct: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Api {
    pub runs: usize,
    pub usd: f64,
}

#[derive(Clone, Debug)]
pub struct Spend {
    pub classes: Vec<Class>,
    pub api: Api,
}

impl Spend {
    pub fn of(screen: &Screen) -> Result<Self, String> {
        let (window_runs, _) = screen.runs.as_ref().map_err(Clone::clone)?;
        let accounts = screen.accounts.as_ref().map_err(Clone::clone)?;
        let week_runs = screen.week_runs.as_ref().map_err(Clone::clone)?;
        let week_usd = |letter: &str, since: &str| -> f64 { week_runs.iter().filter(|run| run.account == format!("{LOGIN_PREFIX}{letter}") && run.started.as_str() >= since).map(|run| run.usd).sum() };
        let capacity: BTreeMap<String, f64> = accounts
            .iter()
            .filter_map(|account| {
                let (used, resets) = (account.week_used?, account.week_resets?);
                let since = Timestamp::from_unix_seconds(resets.unix_seconds() - WEEK_SECONDS).iso();
                let spent = week_usd(&account.account, &since);
                (used >= MIN_USED_PCT && spent > 0.0).then(|| (account.account.clone(), spent / (used / 100.0)))
            })
            .collect();
        let total_capacity: f64 = capacity.values().sum();
        let mut api = Api::default();
        let mut classes: BTreeMap<String, (Class, BTreeMap<String, f64>)> = BTreeMap::new();
        for run in window_runs {
            if run.account == API_ACCOUNT {
                api.runs += 1;
                api.usd += run.usd;
                continue;
            }
            let mix = if run.mix().is_empty() { "unknown".to_owned() } else { run.mix() };
            let (class, by_account) = classes.entry(mix.clone()).or_insert_with(|| (Class { mix, runs: 0, usd: 0.0, week_pct: BTreeMap::new(), all_pct: None }, BTreeMap::new()));
            class.runs += 1;
            class.usd += run.usd;
            if let Some(letter) = run.account.strip_prefix(LOGIN_PREFIX) {
                *by_account.entry(letter.to_owned()).or_default() += run.usd;
            }
        }
        let mut classes: Vec<Class> = classes
            .into_values()
            .map(|(mut class, by_account)| {
                for (letter, usd) in &by_account {
                    if let Some(week) = capacity.get(letter) {
                        class.week_pct.insert(letter.clone(), usd / week * 100.0);
                    }
                }
                for letter in capacity.keys() {
                    class.week_pct.entry(letter.clone()).or_insert(0.0);
                }
                let sized: f64 = by_account.iter().filter(|(letter, _)| capacity.contains_key(*letter)).map(|(_, usd)| usd).sum();
                class.all_pct = (total_capacity > 0.0).then(|| sized / total_capacity * 100.0);
                class
            })
            .collect();
        classes.sort_by(|a, b| b.usd.total_cmp(&a.usd));
        Ok(Self { classes, api })
    }
}
