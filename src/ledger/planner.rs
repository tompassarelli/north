//! `north ledger recommend --planner`: the planner effort band an item earns, read from the
//! threads ledger's past items. Each past item's realized difficulty is ranked against cut points
//! measured from where more planning plausibly paid off; a new item takes the mean difficulty of
//! its nearest past items by facets, summed box ETA and prior Not dones.
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, OpenFlags, Row};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

const CATEGORIES: [&str; 11] = [
    "mechanical", "docs-policy", "tooling", "feature", "bug-known-cause", "debugging-unknown-cause",
    "netcode-determinism", "performance", "balance-tuning", "native-check", "research",
];
const SCOPES: [&str; 4] = ["one-file", "module", "cross-module", "cross-repo"];
const SURFACES: [&str; 8] = ["ts", "rust", "lua", "nix", "shell", "workflow", "docs", "assets"];
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
const BANDS: [Band; 3] = [Band::High, Band::Xhigh, Band::Max];

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bStop:[ \t]*(?:reasoning|decision)\b").expect("stop pattern"));
static PLANNER_TIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*Tier:[ \t]*planner\b").expect("planner tier pattern"));
static FACET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(Category|Scope|Surface):[ \t]*([A-Za-z-]+)").expect("facet pattern"));
static COUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(Summed box ETA|Prior Not dones):[ \t]*(\d+)(?:m)?(?:[ \t,;\n.]|$)").expect("count pattern"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Band {
    High,
    Xhigh,
    Max,
}

impl Band {
    fn name(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// The difficulties that open the xhigh and max bands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cuts {
    pub xhigh: f64,
    pub max: f64,
}

impl Cuts {
    pub(crate) fn band(self, difficulty: f64) -> Band {
        if difficulty < self.xhigh {
            Band::High
        } else if difficulty < self.max {
            Band::Xhigh
        } else {
            Band::Max
        }
    }
}

/// What an item looked like when it was planned.
#[derive(Clone, Debug, Default)]
pub(crate) struct Facets {
    pub category: Option<String>,
    pub scope: Option<String>,
    pub surface: Option<String>,
    pub initial_eta: Option<f64>,
    pub prior_not_done: Option<u32>,
}

/// One past item: its planning-time facets and how hard it turned out.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub item: String,
    pub facets: Facets,
    pub difficulty: f64,
    /// How many signals say a better plan would have cut it: 2+ reasoning or decision Stops, an escalation, wall over 2x ETA.
    pub benefit: usize,
}

struct Run {
    agent: String,
    item: Option<String>,
    follows: Option<String>,
    tier: Option<String>,
    category: Option<String>,
    scope: Option<String>,
    surface: Option<String>,
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
    Ok(Run {
        agent: row.get("agent")?,
        item: row.get("item")?,
        follows: row.get("follows")?,
        tier: row.get("tier")?,
        category: row.get("category")?,
        scope: row.get("scope")?,
        surface: row.get("surface")?,
        began: row.get::<_, Option<String>>("staffed")?.filter(|text| !text.is_empty()).or(row.get::<_, Option<String>>("started")?.filter(|text| !text.is_empty())),
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
    let wall = minutes_between(start, last).max(0.0);
    let eta: f64 = runs.iter().map(|run| run.eta_min.unwrap_or(0.0)).sum();
    let missing_eta = runs.iter().any(|run| run.eta_min.is_none_or(|eta| eta <= 0.0));
    let ratio = (eta > 0.0 && !missing_eta).then(|| wall / eta);
    let not_done = runs.iter().filter(|run| run.outcome.as_deref() == Some("not done")).count();
    let stops = runs.iter().filter(|run| STOP.is_match(&run.report)).count();
    let previous = |run: &Run| run.follows.as_deref().and_then(|follows| by_agent.get(follows).copied());
    let restaffs = runs.iter().filter(|run| previous(run).is_some()).count();
    let escalations = runs
        .iter()
        .filter(|run| {
            run.outcome.as_deref() == Some("escalated")
                || run.arm.as_deref() == Some("escalate")
                || previous(run).is_some_and(|before| matches!((tier_rank(run.tier.as_deref()), tier_rank(before.tier.as_deref())), (Some(now), Some(then)) if now > then))
        })
        .count();
    let compactions: i64 = runs.iter().map(|run| run.compactions).sum();
    let difficulty = [ratio.unwrap_or(0.0), not_done as f64, restaffs as f64, escalations as f64, compactions as f64].into_iter().map(f64::ln_1p).sum();
    let signals = usize::from(stops >= 2) + usize::from(escalations > 0) + usize::from(ratio.is_some_and(|ratio| ratio > 2.0));
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
            initial_eta,
            prior_not_done: prior_not_done(first, start, by_agent),
        },
        difficulty,
        benefit: signals.min(2),
    })
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

fn by_difficulty(a: &Item, b: &Item) -> Ordering {
    a.difficulty.total_cmp(&b.difficulty).then_with(|| a.item.cmp(&b.item))
}

/// The cut points that classify the most items into the band their payoff signals earn: none for
/// high, one for xhigh, two or more for max; among equal fits, the lower efforts reach furthest.
pub(crate) fn cuts(items: &[Item]) -> Cuts {
    let mut rows: Vec<&Item> = items.iter().collect();
    rows.sort_by(|a, b| by_difficulty(a, b));
    let mut prefix = vec![[0usize; 3]];
    for row in &rows {
        let mut counts = prefix[prefix.len() - 1];
        counts[row.benefit] += 1;
        prefix.push(counts);
    }
    let total = prefix[rows.len()];
    let mut boundaries = vec![0];
    boundaries.extend((1..rows.len()).filter(|&index| rows[index - 1].difficulty < rows[index].difficulty));
    boundaries.push(rows.len());
    let mut best = (0, 0, 0);
    for &xhigh in &boundaries {
        for &max in boundaries.iter().filter(|&&max| max >= xhigh) {
            let correct = prefix[xhigh][0] + prefix[max][1] - prefix[xhigh][1] + total[2] - prefix[max][2];
            best = best.max((correct, xhigh, max));
        }
    }
    let cut = |index: usize| rows.get(index).map_or(f64::INFINITY, |row| row.difficulty);
    Cuts { xhigh: cut(best.1), max: cut(best.2) }
}

/// How far a past item sits from the one being sized: one per differing facet, plus a bounded
/// log-ratio of their summed ETAs and a bounded gap in prior Not dones.
fn distance(asked: &Facets, other: &Facets) -> f64 {
    let same = |a: &Option<String>, b: &Option<String>| a.as_deref().is_some_and(|value| value != "unknown" && Some(value) == b.as_deref());
    let facets = [(&asked.category, &other.category), (&asked.scope, &other.scope), (&asked.surface, &other.surface)];
    let mismatched = facets.into_iter().filter(|(a, b)| !same(a, b)).count() as f64;
    let gap = match (asked.initial_eta, other.initial_eta) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => {
            let gap = (a / b).ln().abs();
            gap / (1.0 + gap)
        }
        _ => 1.0,
    };
    let prior = match (asked.prior_not_done, other.prior_not_done) {
        (Some(a), Some(b)) => {
            let gap = f64::from(a.abs_diff(b));
            gap / (1.0 + gap)
        }
        _ => 0.0,
    };
    mismatched + gap + prior
}

fn neighbours(count: usize) -> usize {
    ((count as f64).sqrt().ceil() as usize).max(1)
}

/// The mean realized difficulty of the nearest sqrt(n) past items.
pub(crate) fn predict(asked: &Facets, training: &[Item]) -> f64 {
    let mut ranked: Vec<(f64, &Item)> = training.iter().map(|item| (distance(asked, &item.facets), item)).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.item.cmp(&b.1.item)));
    let near = &ranked[..neighbours(training.len()).min(ranked.len())];
    if near.is_empty() {
        return 0.0;
    }
    near.iter().map(|(_, item)| item.difficulty).sum::<f64>() / near.len() as f64
}

/// Planner runs, those whose brief names `Tier: planner`, counted by the effort they ran at.
fn planner_efforts(db: &Connection) -> rusqlite::Result<BTreeMap<String, usize>> {
    let mut statement = db.prepare("SELECT brief_text, effort, tier FROM runs WHERE brief_text IS NOT NULL")?;
    let mut efforts = BTreeMap::new();
    for row in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?)))? {
        let (brief, effort, tier) = row?;
        if PLANNER_TIER.is_match(&brief) {
            let effort = effort.filter(|effort| !effort.is_empty()).or_else(|| tier.and_then(|tier| tier.split_whitespace().last().map(str::to_owned)));
            *efforts.entry(effort.unwrap_or_else(|| "unknown".into())).or_insert(0) += 1;
        }
    }
    Ok(efforts)
}

/// The planning-time facets a brief states, refused unless it gives all five.
pub(crate) fn asked(text: &str) -> Result<Facets, String> {
    let mut facets = Facets::default();
    for captures in FACET.captures_iter(text) {
        let value = Some(captures[2].to_lowercase());
        let slot = match &captures[1] {
            "Category" => &mut facets.category,
            "Scope" => &mut facets.scope,
            _ => &mut facets.surface,
        };
        if slot.is_none() {
            *slot = value;
        }
    }
    for captures in COUNT.captures_iter(text) {
        let Ok(value) = captures[2].parse::<u32>() else { continue };
        match &captures[1] {
            "Summed box ETA" => facets.initial_eta = facets.initial_eta.or(Some(f64::from(value))),
            _ => facets.prior_not_done = facets.prior_not_done.or(Some(value)),
        }
    }
    if !facets.category.as_deref().is_some_and(|category| CATEGORIES.contains(&category)) {
        return Err(format!("recommend needs `Category: <one of {}>` in {text:?}", CATEGORIES.join(", ")));
    }
    if facets.scope.is_none() || facets.surface.is_none() || facets.initial_eta.is_none() || facets.prior_not_done.is_none() {
        return Err("planner needs Scope, Surface, Summed box ETA and Prior Not dones".into());
    }
    for (name, value, allowed) in [("Scope", &facets.scope, &SCOPES[..]), ("Surface", &facets.surface, &SURFACES[..])] {
        if !value.as_deref().is_some_and(|value| allowed.contains(&value)) {
            return Err(format!("planner needs `{name}: <one of {}>` in {text:?}", allowed.join(", ")));
        }
    }
    if facets.initial_eta.is_some_and(|eta| eta <= 0.0) {
        return Err("Summed box ETA must be positive".into());
    }
    Ok(facets)
}

fn share(count: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { 100.0 * count as f64 / total as f64 }
}

/// The band report for a brief's facets against the threads ledger at `threads_db`, which it only reads.
pub fn recommend(threads_db: &Path, facets: &str) -> NorthResult<String> {
    let asked = asked(facets).map_err(NorthError::Usage)?;
    let refused = |error: rusqlite::Error| NorthError::Configuration(format!("threads ledger {}: {error}", threads_db.display()));
    let db = Connection::open_with_flags(threads_db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(refused)?;
    let items = items(&db).map_err(refused)?;
    if items.is_empty() {
        return Err(NorthError::Usage("planner needs past items".into()));
    }
    let efforts = planner_efforts(&db).map_err(refused)?;
    Ok(report(&asked, &items, &efforts))
}

fn report(asked: &Facets, items: &[Item], efforts: &BTreeMap<String, usize>) -> String {
    let cuts = cuts(items);
    let prediction = predict(asked, items);
    let mut out = String::new();
    let _ = writeln!(out, "planner {}  nearest items n={}  difficulty={prediction:.6}", cuts.band(prediction).name(), neighbours(items.len()));
    let _ = writeln!(out, "cut points high=[0,{0:.6}) xhigh=[{0:.6},{1:.6}) max=[{1:.6},inf)", cuts.xhigh, cuts.max);
    let realized = BANDS.map(|band| items.iter().filter(|item| cuts.band(item.difficulty) == band).count());
    let _ = writeln!(
        out,
        "realized items high/xhigh/max={:.2}/{:.2}/{:.2}% of {}",
        share(realized[0], items.len()),
        share(realized[1], items.len()),
        share(realized[2], items.len()),
        items.len()
    );
    let runs: usize = efforts.values().sum();
    let mut line = format!("planner runs by effort n={runs}:");
    for band in BANDS {
        let count = efforts.get(band.name()).copied().unwrap_or(0);
        let _ = write!(line, " {}={count} ({:.2}%)", band.name(), share(count, runs));
    }
    let other = runs - BANDS.iter().map(|band| efforts.get(band.name()).copied().unwrap_or(0)).sum::<usize>();
    if other > 0 {
        let _ = write!(line, " other={other} ({:.2}%)", share(other, runs));
    }
    let _ = writeln!(out, "{line}");
    if asked.prior_not_done.is_some_and(|prior| prior > 0) {
        let missing = items.iter().filter(|item| item.facets.prior_not_done.is_none()).count();
        let _ = writeln!(
            out,
            "prior Not dones: {} given; history has no planning-time prior Not dones for {missing} of {} items, so matching ignores it for those",
            asked.prior_not_done.unwrap_or(0),
            items.len()
        );
    }
    out
}

#[cfg(test)]
#[path = "planner_tests.rs"]
mod tests;
