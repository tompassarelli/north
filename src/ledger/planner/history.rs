//! Past items read from the threads ledger: what each looked like when it was planned and how
//! hard it turned out across all the runs that name it.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, Row};

use super::Facets;
use crate::foundation::clock::Timestamp;

const TIER_ORDER: [&str; 10] = [
    "claude:haiku", "codex:luna-low", "claude:sonnet-high", "codex:sol-medium", "codex:sol-high", "codex:sol-xhigh",
    "claude:opus-medium", "claude:opus-high", "claude:opus-xhigh", "codex:astra",
];
const RUN_TIERS: [(&str, &str); 10] = [
    ("claude:haiku", "haiku"),
    ("claude:sonnet-high", "sonnet"),
    ("claude:opus-medium", "medium"),
    ("claude:opus-high", "high"),
    ("claude:opus-xhigh", "xhigh"),
    ("codex:luna-low", "gpt-6-luna low"),
    ("codex:sol-medium", "gpt-6.1-sol medium"),
    ("codex:sol-high", "gpt-6.1-sol high"),
    ("codex:sol-xhigh", "gpt-6.1-sol xhigh"),
    ("codex:astra", "gpt-6-astra xhigh"),
];

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bStop:[ \t]*(?:reasoning|decision)\b").expect("stop pattern"));

/// One past item: its planning-time facets and how hard it turned out.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub item: String,
    pub facets: Facets,
    pub realized: Realized,
    pub difficulty: f64,
    /// How many signals say a better plan would have cut it: 2+ reasoning or decision Stops, an escalation, wall over 2x ETA.
    pub benefit: usize,
}

/// The measurements an item's difficulty is summed from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Realized {
    pub runs: usize,
    pub wall_min: f64,
    pub eta_min: f64,
    pub missing_eta: usize,
    pub ratio: Option<f64>,
    pub not_done: usize,
    pub stops: usize,
    pub restaffs: usize,
    pub escalations: usize,
    pub compactions: i64,
}

struct Run {
    agent: String,
    item: Option<String>,
    follows: Option<String>,
    tier: Option<String>,
    category: Option<String>,
    scope: Option<String>,
    surface: Option<String>,
    spec: Option<String>,
    verify: Option<String>,
    began: Option<String>,
    ended: Timestamp,
    minutes: f64,
    eta_min: Option<f64>,
    outcome: Option<String>,
    report: String,
    arm: Option<String>,
    compactions: i64,
}

fn run_row(row: &Row) -> rusqlite::Result<Run> {
    let ended: String = row.get("ended")?;
    let text = |column: &str| row.get::<_, Option<String>>(column).map(|value| value.filter(|text| !text.is_empty()));
    Ok(Run {
        agent: row.get("agent")?,
        item: row.get("item")?,
        follows: row.get("follows")?,
        tier: row.get("tier")?,
        category: row.get("category")?,
        scope: row.get("scope")?,
        surface: row.get("surface")?,
        spec: text("spec")?,
        verify: text("verify")?,
        began: text("staffed")?.or(text("started")?),
        ended: moment(&ended)?,
        minutes: row.get::<_, Option<f64>>("minutes")?.unwrap_or(0.0),
        eta_min: row.get("eta_min")?,
        outcome: row.get("outcome")?,
        report: row.get::<_, Option<String>>("report")?.unwrap_or_default(),
        arm: row.get("arm")?,
        compactions: row.get::<_, Option<i64>>("compactions")?.unwrap_or(0),
    })
}

fn moment(text: &str) -> rusqlite::Result<Timestamp> {
    Timestamp::parse_loose(text).ok_or_else(|| rusqlite::Error::InvalidColumnType(0, format!("time {text:?}"), rusqlite::types::Type::Text))
}

fn minutes_between(from: Timestamp, to: Timestamp) -> f64 {
    (to.unix_millis() - from.unix_millis()) as f64 / 60_000.0
}

fn tier_rank(tier: Option<&str>) -> Option<usize> {
    let tier = tier.map(str::trim).filter(|tier| !tier.is_empty())?;
    let key = RUN_TIERS.iter().find(|(key, run)| *key == tier || *run == tier).map_or(tier, |(key, _)| key);
    TIER_ORDER.iter().position(|ordered| *ordered == key)
}

/// The repository an item reference names, such as `north` for `north#242`.
pub(crate) fn repository(item: &str) -> Option<String> {
    item.split_once('#').map(|(repo, _)| repo.to_lowercase()).filter(|repo| !repo.is_empty())
}

/// Every past item in the ledger, each from all the runs that name it; an item with a run still
/// going has not realized its difficulty and is left out.
pub(crate) fn items(db: &Connection) -> rusqlite::Result<Vec<Item>> {
    let unfinished: BTreeSet<String> = db
        .prepare("SELECT DISTINCT item FROM runs WHERE item IS NOT NULL AND (coalesce(ended, '') = '' OR outcome = 'running')")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut statement = db.prepare(
        "SELECT * FROM runs WHERE coalesce(ended, '') != '' AND outcome IS NOT 'running' ORDER BY coalesce(staffed, started, ended), ended, agent",
    )?;
    let runs = statement.query_map([], run_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    let by_agent: BTreeMap<&str, &Run> = runs.iter().map(|run| (run.agent.as_str(), run)).collect();
    let mut order = Vec::new();
    let mut groups: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    for run in &runs {
        if let Some(item) = run.item.as_deref().filter(|item| !unfinished.contains(*item)) {
            groups.entry(item).or_insert_with(|| {
                order.push(item);
                Vec::new()
            }).push(run);
        }
    }
    order.into_iter().map(|item| realize(item, &groups[item], &by_agent)).collect()
}

fn realize(item: &str, runs: &[&Run], by_agent: &BTreeMap<&str, &Run>) -> rusqlite::Result<Item> {
    let first = runs[0];
    let mut begins = Vec::new();
    for run in runs {
        begins.push(match run.began.as_deref() {
            Some(began) => moment(began)?,
            None => Timestamp::from_unix_millis(run.ended.unix_millis() - (run.minutes * 60_000.0) as i64),
        });
    }
    let start = begins.iter().copied().min_by_key(|at| at.unix_millis()).unwrap_or(first.ended);
    let last = runs.iter().map(|run| run.ended).max_by_key(|at| at.unix_millis()).unwrap_or(first.ended);
    let first_outcome = runs.iter().map(|run| run.ended).min_by_key(|at| at.unix_millis()).unwrap_or(first.ended);
    let wall_min = minutes_between(start, last).max(0.0);
    let eta_min: f64 = runs.iter().map(|run| run.eta_min.unwrap_or(0.0)).sum();
    let missing_eta = runs.iter().filter(|run| run.eta_min.is_none_or(|eta| eta <= 0.0)).count();
    let previous = |run: &Run| run.follows.as_deref().and_then(|follows| by_agent.get(follows).copied());
    let escalated = |run: &&&Run| {
        run.outcome.as_deref() == Some("escalated")
            || run.arm.as_deref() == Some("escalate")
            || previous(run).is_some_and(|before| matches!((tier_rank(run.tier.as_deref()), tier_rank(before.tier.as_deref())), (Some(now), Some(then)) if now > then))
    };
    let realized = Realized {
        runs: runs.len(),
        wall_min,
        eta_min,
        missing_eta,
        ratio: (eta_min > 0.0 && missing_eta == 0).then(|| wall_min / eta_min),
        not_done: runs.iter().filter(|run| run.outcome.as_deref() == Some("not done")).count(),
        stops: runs.iter().filter(|run| STOP.is_match(&run.report)).count(),
        restaffs: runs.iter().filter(|run| previous(run).is_some()).count(),
        escalations: runs.iter().filter(escalated).count(),
        compactions: runs.iter().map(|run| run.compactions).sum(),
    };
    let mut initial = Vec::new();
    for (run, began) in runs.iter().zip(&begins) {
        if run.began.is_some() && began.unix_millis() < first_outcome.unix_millis() {
            initial.push(run.eta_min.unwrap_or(0.0));
        }
    }
    let initial_eta = if initial.is_empty() { first.eta_min } else { Some(initial.iter().sum()) };
    Ok(Item {
        item: item.to_owned(),
        facets: Facets {
            category: first.category.clone(),
            scope: first.scope.clone(),
            surface: first.surface.clone(),
            spec: first.spec.clone(),
            verify: first.verify.clone(),
            repository: repository(item),
            initial_eta,
            prior_not_done: prior_not_done(first, start, by_agent),
        },
        difficulty: difficulty(&realized),
        benefit: benefit(&realized),
        realized,
    })
}

fn difficulty(realized: &Realized) -> f64 {
    let Realized { ratio, not_done, restaffs, escalations, compactions, .. } = *realized;
    [ratio.unwrap_or(0.0), not_done as f64, restaffs as f64, escalations as f64, compactions as f64].into_iter().map(f64::ln_1p).sum()
}

fn benefit(realized: &Realized) -> usize {
    let signals = usize::from(realized.stops >= 2) + usize::from(realized.escalations > 0) + usize::from(realized.ratio.is_some_and(|ratio| ratio > 2.0));
    signals.min(2)
}

/// Not dones on the item's Follows chain that ended before it began, or None when the chain is broken.
fn prior_not_done(first: &Run, start: Timestamp, by_agent: &BTreeMap<&str, &Run>) -> Option<u32> {
    let mut ancestor = first.follows.as_deref().filter(|follows| !follows.is_empty())?;
    let mut seen = BTreeSet::new();
    let mut count = 0;
    loop {
        let previous = by_agent.get(ancestor).filter(|previous| previous.ended.unix_millis() <= start.unix_millis())?;
        if !seen.insert(ancestor) {
            return None;
        }
        count += u32::from(previous.outcome.as_deref() == Some("not done"));
        match previous.follows.as_deref().filter(|next| !next.is_empty()) {
            Some(next) => ancestor = next,
            None => return Some(count),
        }
    }
}
