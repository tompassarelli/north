//! `north ledger recommend --planner`: the planner effort band an item earns, read from the
//! threads ledger's past items. Each past item's realized difficulty is ranked against cut points
//! measured from where more planning plausibly paid off; a new item takes the band most of its
//! nearest past items realized, matched on the planning-time inputs in [`SHIPPED`].
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, OpenFlags};

use crate::error::{NorthError, NorthResult};

mod history;
mod readout;

pub(crate) use history::{Item, items};
pub use readout::band_readout;

const CATEGORIES: [&str; 11] = [
    "mechanical", "docs-policy", "tooling", "feature", "bug-known-cause", "debugging-unknown-cause",
    "netcode-determinism", "performance", "balance-tuning", "native-check", "research",
];
const SCOPES: [&str; 4] = ["one-file", "module", "cross-module", "cross-repo"];
const SURFACES: [&str; 8] = ["ts", "rust", "lua", "nix", "shell", "workflow", "docs", "assets"];
const BANDS: [Band; 3] = [Band::High, Band::Xhigh, Band::Max];

static PLANNER_TIER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[ \t]*Tier:[ \t]*planner\b").expect("planner tier pattern"));
static FACET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(Category|Scope|Surface|Spec|Verify):[ \t]*([A-Za-z-]+)").expect("facet pattern"));
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bItem:[ \t]*([A-Za-z0-9_.-]+#\d+)").expect("item pattern"));
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
    pub spec: Option<String>,
    pub verify: Option<String>,
    pub repository: Option<String>,
    pub initial_eta: Option<f64>,
    pub prior_not_done: Option<u32>,
}

/// One planning-time input the nearest-item match can weigh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    Category,
    Scope,
    Surface,
    Spec,
    Verify,
    Repository,
    Eta,
    PriorNotDone,
}

/// A named set of inputs that leave-one-out compares against the others.
pub(crate) struct InputSet {
    pub name: &'static str,
    pub inputs: &'static [Input],
}

/// The inputs ed063ad7's rule matched on, by the mean difficulty of the nearest items.
pub(crate) const PREVIOUS: InputSet = InputSet {
    name: "category scope surface eta prior",
    inputs: &[Input::Category, Input::Scope, Input::Surface, Input::Eta, Input::PriorNotDone],
};

/// The inputs `recommend` matches on, chosen by `planner-band`'s leave-one-out table.
pub(crate) const SHIPPED: InputSet = InputSet {
    name: "category scope surface eta prior repository",
    inputs: &[Input::Category, Input::Scope, Input::Surface, Input::Eta, Input::PriorNotDone, Input::Repository],
};

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

fn bounded(gap: f64) -> f64 {
    gap / (1.0 + gap)
}

impl Facets {
    /// The value this item gives for a named input, or None for the numeric ones.
    fn named(&self, input: Input) -> Option<&Option<String>> {
        match input {
            Input::Category => Some(&self.category),
            Input::Scope => Some(&self.scope),
            Input::Surface => Some(&self.surface),
            Input::Spec => Some(&self.spec),
            Input::Verify => Some(&self.verify),
            Input::Repository => Some(&self.repository),
            Input::Eta | Input::PriorNotDone => None,
        }
    }

    /// A bounded log-ratio of summed ETAs, the full 1 when either is unknown.
    fn eta_gap(&self, other: &Self) -> f64 {
        match (self.initial_eta, other.initial_eta) {
            (Some(a), Some(b)) if a > 0.0 && b > 0.0 => bounded((a / b).ln().abs()),
            _ => 1.0,
        }
    }

    /// A bounded gap in prior Not dones, none when either is unknown.
    fn prior_gap(&self, other: &Self) -> f64 {
        match (self.prior_not_done, other.prior_not_done) {
            (Some(a), Some(b)) => bounded(f64::from(a.abs_diff(b))),
            _ => 0.0,
        }
    }
}

/// How far a past item sits from the one being sized over `inputs`: one per differing named
/// value, a bounded log-ratio of their summed ETAs and a bounded gap in prior Not dones.
fn distance(asked: &Facets, other: &Facets, inputs: &[Input]) -> f64 {
    let same = |a: &Option<String>, b: &Option<String>| a.as_deref().is_some_and(|value| value != "unknown" && Some(value) == b.as_deref());
    let term = |input: Input| match (asked.named(input), other.named(input)) {
        (Some(a), Some(b)) => if same(a, b) { 0.0 } else { 1.0 },
        _ if input == Input::Eta => asked.eta_gap(other),
        _ => asked.prior_gap(other),
    };
    inputs.iter().map(|input| term(*input)).sum()
}

fn neighbours(count: usize) -> usize {
    ((count as f64).sqrt().ceil() as usize).max(1)
}

/// The nearest sqrt(n) past items over `inputs`, ties broken by item reference.
fn nearest<'a>(asked: &Facets, training: impl IntoIterator<Item = &'a Item>, inputs: &[Input]) -> Vec<&'a Item> {
    let mut ranked: Vec<(f64, &Item)> = training.into_iter().map(|item| (distance(asked, &item.facets, inputs), item)).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.item.cmp(&b.1.item)));
    ranked.truncate(neighbours(ranked.len()));
    ranked.into_iter().map(|(_, item)| item).collect()
}

/// The mean realized difficulty of the nearest past items over `inputs`.
pub(crate) fn predict<'a>(asked: &Facets, training: impl IntoIterator<Item = &'a Item>, inputs: &[Input]) -> f64 {
    let near = nearest(asked, training, inputs);
    if near.is_empty() {
        return 0.0;
    }
    near.iter().map(|item| item.difficulty).sum::<f64>() / near.len() as f64
}

/// The band most of the nearest past items realized under `cuts`, ties going to the lower effort.
pub(crate) fn vote<'a>(asked: &Facets, training: impl IntoIterator<Item = &'a Item>, inputs: &[Input], cuts: Cuts) -> Band {
    let near = nearest(asked, training, inputs);
    let votes = BANDS.map(|band| near.iter().filter(|item| cuts.band(item.difficulty) == band).count());
    BANDS[(0..BANDS.len()).max_by_key(|&slot| (votes[slot], usize::MAX - slot)).unwrap_or(0)]
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

/// Every facet a brief states, the first value of each kept.
fn stated(text: &str) -> Facets {
    let mut facets = Facets::default();
    for captures in FACET.captures_iter(text) {
        let value = Some(captures[2].to_lowercase());
        let slot = match &captures[1] {
            "Category" => &mut facets.category,
            "Scope" => &mut facets.scope,
            "Surface" => &mut facets.surface,
            "Spec" => &mut facets.spec,
            _ => &mut facets.verify,
        };
        if slot.is_none() {
            *slot = value;
        }
    }
    facets.repository = ITEM.captures(text).and_then(|captures| history::repository(&captures[1]));
    for captures in COUNT.captures_iter(text) {
        let Ok(value) = captures[2].parse::<u32>() else { continue };
        match &captures[1] {
            "Summed box ETA" => facets.initial_eta = facets.initial_eta.or(Some(f64::from(value))),
            _ => facets.prior_not_done = facets.prior_not_done.or(Some(value)),
        }
    }
    facets
}

/// The planning-time facets a brief states, refused unless it gives every one the shipped inputs need.
pub(crate) fn asked(text: &str) -> Result<Facets, String> {
    let facets = stated(text);
    if !facets.category.as_deref().is_some_and(|category| CATEGORIES.contains(&category)) {
        return Err(format!("recommend needs `Category: <one of {}>` in {text:?}", CATEGORIES.join(", ")));
    }
    if facets.scope.is_none() || facets.surface.is_none() || facets.initial_eta.is_none() || facets.prior_not_done.is_none() || facets.repository.is_none() {
        return Err("planner needs Item, Scope, Surface, Summed box ETA and Prior Not dones".into());
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
    let near = nearest(asked, items, SHIPPED.inputs);
    let votes = BANDS.map(|band| near.iter().filter(|item| cuts.band(item.difficulty) == band).count());
    let mut out = String::new();
    let _ = writeln!(
        out,
        "planner {}  nearest items n={} by {}  votes high/xhigh/max={}/{}/{}",
        vote(asked, items, SHIPPED.inputs, cuts).name(),
        near.len(),
        SHIPPED.name,
        votes[0],
        votes[1],
        votes[2]
    );
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
mod tests;
