use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

use crate::router::pricing::PriceTable;

const LEADS: [&str; 2] = ["parent", "proxy"];
pub(super) const WORKER: &str = "worker";

#[derive(Clone, Debug, PartialEq)]
pub struct SpendRun {
    pub agent: String,
    pub item: Option<String>,
    pub role: String,
    pub tier: String,
    pub category: String,
    pub arm: String,
    pub box_text: Option<String>,
    pub model: Option<String>,
    pub effort: String,
    pub started: String,
    pub ended: String,
    pub landed: bool,
    pub reworked: bool,
    pub input: i64,
    pub cache_read: i64,
    pub output: i64,
    pub skills: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ArmBox {
    pub item: String,
    pub tier: String,
    pub arm: String,
    pub usd: f64,
    #[serde(default)]
    pub category: String,
    #[serde(default = "closed_by_default")]
    pub closed: bool,
    #[serde(default)]
    pub reworked: bool,
    #[serde(default)]
    pub verified: bool,
    /// The reading `verified` replaces: every ticked box of the issue stamped, whichever box the run's brief named.
    #[serde(default)]
    pub issue_verified: bool,
    #[serde(default)]
    pub box_text: Option<String>,
}

fn closed_by_default() -> bool {
    true
}

/// A closed box with what it cost and the worker runs that cost it, in the order they started.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClosedBox {
    pub item: String,
    pub usd: f64,
    pub runs: Vec<BoxRun>,
    /// Runs whose brief named no box on an issue of several boxes: their cost belongs to no one box, so no cell's median reads it.
    #[serde(default)]
    pub unattributed: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct BoxRun {
    pub agent: String,
    pub tier: String,
    pub category: String,
    pub started: String,
    pub usd: f64,
}

/// One box the window's worker runs spent on, closed or not, with the skills any of its runs loaded.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SkillBox {
    pub item: String,
    pub category: String,
    pub usd: f64,
    pub closed: bool,
    pub skills: BTreeSet<String>,
}

impl SkillBox {
    fn of(item: &str, category: &str, usd: f64, closed: bool, runs: &[&SpendRun]) -> Self {
        Self { item: item.to_owned(), category: category.to_owned(), usd, closed, skills: runs.iter().flat_map(|run| run.skills.iter().cloned()).collect() }
    }
}

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Cell {
    pub tier: String,
    pub category: String,
    pub closed: usize,
    pub usd: f64,
    pub staffed: bool,
    pub unclosed_usd: f64,
    pub reworked: usize,
    pub runs: usize,
    boxes: Vec<ClosedBox>,
    run_minutes: Vec<f64>,
}

impl Cell {
    #[doc(hidden)]
    pub fn priced(tier: &str, category: &str, closed: usize, usd: f64) -> Self {
        Self { tier: tier.into(), category: category.into(), closed, usd, staffed: true, unclosed_usd: 0.0, reworked: 0, runs: 0, boxes: Vec::new(), run_minutes: Vec::new() }
    }

    pub fn usd_per_closed(&self) -> f64 {
        (self.usd + self.unclosed_usd) / self.closed as f64
    }

    pub fn kept(&self) -> usize {
        self.closed - self.reworked
    }

    fn attributed_spends(&self) -> Vec<f64> {
        self.boxes.iter().filter_map(|closed| (!closed.unattributed).then_some(closed.usd)).collect()
    }

    /// How many closed boxes of the cell the medians leave out because their runs named no box on an issue of several.
    pub fn unattributed(&self) -> usize {
        self.boxes.len() - self.attributed_spends().len()
    }

    /// The median cost of one closed box of the cell, which one umbrella issue with hundreds of runs does not move; None when no box is recorded.
    pub fn median_box_usd(&self) -> Option<f64> {
        let mut spends = self.attributed_spends();
        (!spends.is_empty()).then(|| percentile(&mut spends, 50))
    }

    /// The median cost of the cell's closed boxes without its dearest one, which shows whether the median rests on more than that one box; None when fewer than two boxes are recorded.
    pub fn median_box_usd_without_dearest(&self) -> Option<f64> {
        let mut spends = self.attributed_spends();
        spends.sort_by(f64::total_cmp);
        spends.pop()?;
        (!spends.is_empty()).then(|| percentile(&mut spends, 50))
    }

    /// The closed boxes of the cell with what each cost, dearest first.
    pub fn boxes_dearest_first(&self) -> Vec<ClosedBox> {
        let mut boxes = self.boxes.clone();
        boxes.sort_by(|a, b| b.usd.total_cmp(&a.usd));
        boxes
    }

    pub fn runs_since(&self) -> Option<&str> {
        self.boxes.iter().flat_map(|closed| closed.runs.iter().map(|run| run.started.as_str())).filter(|started| !started.is_empty()).min()
    }
}

/// What a closed-box cell says about its next box: the median and p90 cost of a closed box and the p75 wall
/// of a run on one, each with the sample it comes from and its 95% order-statistic interval.
#[derive(Clone, Debug, PartialEq)]
pub struct Figures {
    pub closed: usize,
    pub median_usd: f64,
    pub p90_usd: f64,
    pub usd_interval: (f64, f64),
    pub runs: usize,
    pub p75_minutes: f64,
    pub minutes_interval: (f64, f64),
}

/// The nearest-rank `percent` percentile of `values`, which it sorts; `values` must not be empty.
pub fn percentile(values: &mut [f64], percent: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() * percent).div_ceil(100).max(1) - 1]
}

/// The distribution-free 95% interval of a quantile: the sorted sample's values at the 1-based
/// ranks ⌊n·q − 1.96·√(n·q·(1−q))⌋ and ⌈n·q + 1.96·√(n·q·(1−q))⌉, clamped to the sample.
fn interval(values: &mut [f64], percent: usize) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    let (n, q) = (values.len() as f64, percent as f64 / 100.0);
    let spread = 1.96 * (n * q * (1.0 - q)).sqrt();
    let rank = |at: f64| (at.clamp(1.0, n) as usize) - 1;
    (values[rank((n * q - spread).floor())], values[rank((n * q + spread).ceil())])
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct CellBox {
    pub item: String,
    pub category: String,
    pub cell: String,
    pub usd: f64,
    #[serde(default = "closed_by_default")]
    pub closed: bool,
    #[serde(default)]
    pub reworked: bool,
}

#[derive(Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ClosedReport {
    pub cells: Vec<Cell>,
    pub cell_boxes: Vec<CellBox>,
    /// Repositories outside the catalog whose landed runs could not be matched to a closed issue, with the count of those runs.
    pub skipped: Vec<(String, usize)>,
    pub arms: Vec<ArmBox>,
    pub boxes: Vec<SkillBox>,
    /// The open issues whose runs created children in the window; their spend is counted here, not as spend that closed nothing, because the children carry the closing.
    #[serde(default)]
    pub splits: Vec<super::leaf::SplitBox>,
    /// The runs of every role that ended in the window and loaded each skill, by skill.
    pub loaded: BTreeMap<String, usize>,
    pub closed: usize,
    pub spend_usd: f64,
    pub leads_usd: f64,
    /// The rework judge whose verdicts marked the report's boxes reworked.
    pub judge: u32,
}

impl SpendRun {
    fn minutes(&self) -> Option<f64> {
        let wall = Timestamp::parse(&self.ended).ok()?.unix_seconds() - Timestamp::parse(&self.started).ok()?.unix_seconds();
        (wall >= 0).then_some(wall as f64 / 60.0)
    }

    pub(super) fn usd(&self, prices: &PriceTable) -> f64 {
        prices.run_usd(self.model.as_deref(), &self.tier, self.input, self.cache_read, self.output)
    }
}

pub fn checker_verified(body: &str) -> bool {
    let ticked: Vec<&str> = ticked(body).collect();
    !ticked.is_empty() && ticked.iter().all(|line| crate::platform::box_oracles::own_command_passed(line))
}

/// Whether the ticked box of `body` that `box_text` names carries the checker's stamp. A box matches by the slug a claim keeps, so one stored shorter than the slug limit names every box it begins.
pub fn box_verified(body: &str, box_text: &str) -> bool {
    let named = ticked_named(body, box_text);
    !named.is_empty() && named.iter().all(|line| crate::platform::box_oracles::own_command_passed(line))
}

fn ticked_named<'a>(body: &'a str, box_text: &str) -> Vec<&'a str> {
    let Some(wanted) = crate::threads::claims::box_slug(box_text) else { return Vec::new() };
    ticked(body).filter(|line| crate::threads::claims::box_slug(line.trim_start_matches(['-', '*', ' ']).trim_start_matches("[x]").trim_start_matches("[X]")).is_some_and(|slug| slug.starts_with(&wanted))).collect()
}

fn ticked(body: &str) -> impl Iterator<Item = &str> {
    body.lines().map(str::trim_start).filter(|line| line.starts_with("- [x]") || line.starts_with("- [X]"))
}

fn day(started: Option<&str>) -> &str {
    started.map_or("-", |started| started.get(..10).unwrap_or(started))
}

pub fn tier_label(tier: &str) -> String {
    match tier {
        "low" | "medium" | "high" | "xhigh" | "max" => format!("opus-{tier}"),
        other => other.to_owned(),
    }
}

pub fn cell_name(model: Option<&str>, effort: &str) -> Option<String> {
    let model = model?.to_lowercase();
    let family = ["haiku", "sonnet", "opus", "fable"].into_iter().find(|family| model.contains(family))?;
    ["low", "medium", "high", "xhigh", "max"].contains(&effort).then(|| format!("{family}-{effort}"))
}

pub fn summarize(runs: &[SpendRun], closed: &HashSet<String>, bodies: &HashMap<String, String>, since: &str, prices: &PriceTable) -> ClosedReport {
    let mut groups: BTreeMap<(&str, Option<String>), Vec<&SpendRun>> = BTreeMap::new();
    for run in runs.iter().filter(|run| run.role == WORKER) {
        if let Some(item) = run.item.as_deref() {
            groups.entry((item, run.box_text.as_deref().and_then(crate::threads::claims::box_slug))).or_default().push(run);
        }
    }
    let counts = |item: &str, runs: &[&SpendRun]| {
        runs.iter().any(|run| run.landed)
            && (closed.contains(item)
                || runs.iter().any(|run| run.landed && run.ended.as_str() >= since && run.box_text.as_deref().is_some_and(|text| bodies.get(item).is_some_and(|body| !ticked_named(body, text).is_empty()))))
    };
    let (mut boxes, open): (BTreeMap<_, _>, BTreeMap<_, _>) = groups.into_iter().partition(|((item, _), runs)| counts(item, runs));
    let staffed: HashSet<String> = runs.iter().filter(|run| run.role == WORKER && run.started.as_str() >= since).map(|run| tier_label(&run.tier)).collect();
    let mut cells: HashMap<(String, String), Cell> = HashMap::new();
    let mut cell_boxes = Vec::new();
    let mut arms = Vec::new();
    let mut spent_boxes = Vec::new();
    let mut distinct_closed = 0;
    for ((item, slug), runs) in boxes.iter_mut() {
        let unattributed = slug.is_none() && bodies.get(*item).map(String::as_str).is_some_and(crate::threads::claims::several_boxes);
        runs.sort_by(|a, b| (a.started.as_str(), a.ended.as_str()).cmp(&(b.started.as_str(), b.ended.as_str())));
        let category = runs.iter().map(|run| run.category.as_str()).find(|category| *category != "unknown").unwrap_or("unknown");
        let reworked = runs.iter().any(|run| run.landed && run.reworked);
        let mut shares: BTreeMap<(String, String), Vec<&SpendRun>> = BTreeMap::new();
        for run in runs.iter() {
            let own = if run.category == "unknown" { category } else { run.category.as_str() };
            shares.entry((tier_label(&run.tier), own.to_owned())).or_default().push(run);
        }
        let mut spent = 0.0;
        for (key, share) in shares {
            let cell = cells.entry(key.clone()).or_insert(Cell { staffed: staffed.contains(&key.0), tier: key.0, category: key.1, closed: 0, usd: 0.0, unclosed_usd: 0.0, reworked: 0, runs: 0, boxes: Vec::new(), run_minutes: Vec::new() });
            let priced: Vec<BoxRun> = share.iter().map(|run| BoxRun { agent: run.agent.clone(), tier: tier_label(&run.tier), category: run.category.clone(), started: run.started.clone(), usd: run.usd(prices) }).collect();
            let part = priced.iter().map(|run| run.usd).sum::<f64>();
            spent += part;
            cell.closed += 1;
            cell.runs += share.len();
            cell.reworked += usize::from(reworked);
            cell.usd += part;
            cell.boxes.push(ClosedBox { item: (*item).to_owned(), usd: part, runs: priced, unattributed });
            cell.run_minutes.extend(share.iter().filter_map(|run| run.minutes()));
        }
        distinct_closed += 1;
        spent_boxes.push(SkillBox::of(item, category, spent, true, runs));
        if !runs[0].arm.is_empty() {
            arms.push(ArmBox { item: (*item).to_owned(), tier: tier_label(&runs[0].tier), arm: runs[0].arm.clone(), usd: spent, category: category.to_owned(), closed: true, reworked, verified: false, issue_verified: false, box_text: runs[0].box_text.clone() });
        }
        if let Some(cell) = cell_name(runs[0].model.as_deref(), &runs[0].effort) {
            cell_boxes.push(CellBox { item: (*item).to_owned(), category: category.to_owned(), cell, usd: spent, closed: true, reworked });
        }
    }
    for ((item, _), mut runs) in open {
        runs.sort_by(|a, b| (a.started.as_str(), a.ended.as_str()).cmp(&(b.started.as_str(), b.ended.as_str())));
        if runs[0].started.as_str() < since {
            continue;
        }
        let category = runs.iter().map(|run| run.category.as_str()).find(|category| *category != "unknown").unwrap_or("unknown");
        let spent = runs.iter().map(|run| run.usd(prices)).sum::<f64>();
        spent_boxes.push(SkillBox::of(item, category, spent, false, &runs));
        if let Some(cell) = cells.get_mut(&(tier_label(&runs[0].tier), category.to_owned())) {
            cell.unclosed_usd += spent;
        }
        if !runs[0].arm.is_empty() {
            arms.push(ArmBox { item: (*item).to_owned(), tier: tier_label(&runs[0].tier), arm: runs[0].arm.clone(), usd: spent, category: category.to_owned(), closed: false, reworked: false, verified: false, issue_verified: false, box_text: runs[0].box_text.clone() });
        }
        if let Some(cell) = cell_name(runs[0].model.as_deref(), &runs[0].effort) {
            cell_boxes.push(CellBox { item: (*item).to_owned(), category: category.to_owned(), cell, usd: spent, closed: false, reworked: false });
        }
    }
    let mut cells: Vec<Cell> = cells.into_values().collect();
    cells.sort_by(|a, b| (&a.tier, &a.category).cmp(&(&b.tier, &b.category)));
    let window = || runs.iter().filter(|run| run.ended.as_str() >= since);
    let mut loaded = BTreeMap::new();
    for skill in window().flat_map(|run| &run.skills) {
        *loaded.entry(skill.clone()).or_default() += 1;
    }
    ClosedReport {
        judge: crate::threads::calibration::rework::JUDGE_VERSION,
        skipped: Vec::new(),
        closed: distinct_closed,
        cells,
        cell_boxes,
        arms,
        boxes: spent_boxes,
        splits: Vec::new(),
        loaded,
        spend_usd: window().map(|run| run.usd(prices)).sum(),
        leads_usd: window().filter(|run| LEADS.contains(&run.role.as_str())).map(|run| run.usd(prices)).sum(),
    }
}

impl ClosedReport {
    /// Marks each closed arm box verified when `corroborated`, the box nodes the graph's `corroborated` rule holds by `checker`, names the box its run's brief named; only where no graph exists, by the stamp on that box, or by every ticked box of the issue `bodies` holds when the run named none. `issue_verified` is the issue-level stamp read alone.
    pub fn verified(mut self, bodies: &HashMap<String, String>, corroborated: Option<&HashSet<String>>) -> Self {
        for arm in &mut self.arms {
            let body = bodies.get(&arm.item).filter(|_| arm.closed);
            arm.issue_verified = body.is_some_and(|body| checker_verified(body));
            arm.verified = match corroborated {
                Some(boxes) => arm.closed && arm.box_text.as_deref().and_then(|text| crate::threads::claims::box_node(&arm.item, text)).is_some_and(|node| boxes.contains(&node)),
                None => body.is_some_and(|body| arm.box_text.as_deref().map_or(arm.issue_verified, |text| box_verified(body, text))),
            };
        }
        self
    }

    fn per_closed(&self, usd: f64) -> Option<f64> {
        (self.closed > 0).then_some(usd / self.closed as f64)
    }

    pub fn all_usd_per_closed(&self) -> Option<f64> {
        self.per_closed(self.spend_usd)
    }

    pub fn leads_usd_per_closed(&self) -> Option<f64> {
        self.per_closed(self.leads_usd)
    }

    /// The share of the closed boxes' spend in `categories` that cells whose tier label names
    /// `family` hold; None when those categories closed no box.
    pub fn family_share(&self, family: &str, categories: &[String]) -> Option<f64> {
        let cells = || self.cells.iter().filter(|cell| categories.contains(&cell.category));
        let total: f64 = cells().map(|cell| cell.usd).sum();
        (total > 0.0).then(|| cells().filter(|cell| cell.tier.contains(family)).map(|cell| cell.usd).sum::<f64>() / total)
    }

    /// The median cost of one closed box across every cell, and how many boxes closed; None when
    /// none did.
    pub fn median_box_usd(&self) -> Option<(f64, usize)> {
        let mut spends: Vec<f64> = self.cells.iter().flat_map(|cell| cell.boxes.iter().map(|closed| closed.usd)).collect();
        (!spends.is_empty()).then(|| (percentile(&mut spends, 50), spends.len()))
    }

    /// The figures of every cell in `category` whose tier label names `family` (`haiku`, `sonnet` or
    /// `opus`), pooled; None when no box closed there or no run of it has a wall time.
    pub fn figures(&self, family: &str, category: &str) -> Option<Figures> {
        let cells: Vec<&Cell> = self.cells.iter().filter(|cell| cell.category == category && cell.tier.contains(family)).collect();
        let mut spends: Vec<f64> = cells.iter().flat_map(|cell| cell.boxes.iter().map(|closed| closed.usd)).collect();
        let mut walls: Vec<f64> = cells.iter().flat_map(|cell| cell.run_minutes.iter().copied()).collect();
        (!spends.is_empty() && !walls.is_empty()).then(|| Figures {
            closed: spends.len(),
            median_usd: percentile(&mut spends, 50),
            p90_usd: percentile(&mut spends, 90),
            usd_interval: interval(&mut spends, 50),
            runs: walls.len(),
            p75_minutes: percentile(&mut walls, 75),
            minutes_interval: interval(&mut walls, 75),
        })
    }

    /// Worker runs per closed box in each category, pooled over its tiers: (category, runs, closed boxes).
    pub fn runs_per_closed(&self) -> Vec<(String, usize, usize)> {
        let mut categories: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for cell in &self.cells {
            categories.entry(&cell.category).or_default().0 += cell.runs;
        }
        for closed in self.boxes.iter().filter(|closed| closed.closed) {
            categories.entry(&closed.category).or_default().1 += 1;
        }
        categories.into_iter().filter(|(_, (_, closed))| *closed > 0).map(|(category, (runs, closed))| (category.to_owned(), runs, closed)).collect()
    }

    pub fn runs_line(&self, window: &str) -> String {
        let parts: Vec<String> = self.runs_per_closed().iter().map(|(category, runs, closed)| format!("{category} {:.2} ({runs}/{closed})", *runs as f64 / *closed as f64)).collect();
        format!("runs/closed {window}: {}", if parts.is_empty() { "-".to_owned() } else { parts.join(" | ") })
    }

    pub fn runs_since(&self) -> Option<&str> {
        self.cells.iter().filter_map(Cell::runs_since).min()
    }

    fn reach(&self, window: &str) -> String {
        format!("boxes closed in {window}, each priced at API-equivalent rates over all its runs, the earliest started {}", day(self.runs_since()))
    }

    pub fn dashboard(&self, window: &str) -> String {
        let money = |usd: Option<f64>| usd.map_or_else(|| "-".to_owned(), |usd| format!("${usd:.2}"));
        let mut parts = vec![format!("window spend {} ({})", money(self.per_closed(self.spend_usd)), self.closed)];
        parts.extend(self.cells.iter().map(|cell| format!("{} {} {} ({})", cell.tier, cell.category, money(Some(cell.usd_per_closed())), cell.closed)));
        parts.push(format!("leads {}/closed", money(self.per_closed(self.leads_usd))));
        if !self.splits.is_empty() {
            parts.push(format!("split {} (${:.2})", self.splits.len(), self.splits.iter().map(|split| split.usd).sum::<f64>()));
        }
        format!("cost/closed, {}: {}", self.reach(window), parts.join(" | "))
    }

    pub fn json(&self, window: &str) -> Value {
        json!({
            "since": window,
            "runs_since": self.runs_since(),
            "closed": self.closed,
            "all_usd_per_closed": self.per_closed(self.spend_usd),
            "leads_usd_per_closed": self.per_closed(self.leads_usd),
            "cells": self.cells.iter().map(|cell| json!({
                "tier": cell.tier, "category": cell.category, "closed": cell.closed, "usd": cell.usd, "unclosed_usd": cell.unclosed_usd, "reworked": cell.reworked, "usd_per_closed": cell.usd_per_closed(), "runs_since": cell.runs_since(),
                "boxes": cell.boxes.iter().map(|closed| json!({"item": closed.item, "usd": closed.usd, "runs": closed.runs.iter().map(|run| json!({"agent": run.agent, "tier": run.tier, "category": run.category, "started": run.started, "usd": run.usd})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "runs_per_closed": self.runs_per_closed().iter().map(|(category, runs, closed)| json!({"category": category, "runs": runs, "closed": closed, "runs_per_closed": *runs as f64 / *closed as f64})).collect::<Vec<_>>(),
            "arms": self.arms,
            "splits": self.splits.iter().map(|split| json!({"item": split.item, "usd": split.usd})).collect::<Vec<_>>(),
            "skipped_repositories": self.skipped.iter().map(|(repo, runs)| json!({"repo": repo, "landed_runs": runs})).collect::<Vec<_>>(),
        })
    }

    /// One line naming the repositories whose landed runs the report left out, or None when it left out none.
    pub fn skipped_line(&self) -> Option<String> {
        let named: Vec<String> = self.skipped.iter().map(|(repo, runs)| format!("{repo} ({runs} landed runs)")).collect();
        (!named.is_empty()).then(|| format!("skipped repositories the catalog does not know, so their boxes are not counted: {}", named.join(", ")))
    }

    pub fn print_table(&self) {
        println!("tier\tcategory\tclosed\tusd\tunclosed_usd\tusd_per_closed\truns_since");
        for cell in &self.cells {
            println!("{}\t{}\t{}\t{:.2}\t{:.2}\t{:.2}\t{}", cell.tier, cell.category, cell.closed, cell.usd, cell.unclosed_usd, cell.usd_per_closed(), day(cell.runs_since()));
        }
    }
}

