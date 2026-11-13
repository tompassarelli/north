//! Which worker tier a launch starts on: the cheapest its category allows. Haiku for mechanical work
//! with its retry on Sonnet, Opus for the categories in `OPUS`, and Sonnet at high effort for art and
//! every other category. An unknown-cause bug that names a failing test or repro command starts on
//! Sonnet and escalates once to Opus medium; one naming neither starts on Opus medium. A plan runs at
//! Opus high when its scope spans modules, else at medium. Every launch's model and effort then come
//! from one of its category's three live cells (`super::cells`), drawn from the shares below; no route
//! starts at max effort. A local box on no cell arm takes one auto-compact window of
//! `CONTEXT_WINDOWS`. Every arm is a seeded draw from the shares `Shares` reads off the ledger, the
//! same shares `north dash --closed` prints: equal while the arms hold no boxes, then
//! Thompson sampling on cost per kept box with a floor per arm.
use super::cells::{self, Cell};
use super::facets;
use super::issues::Issue;
use super::record::Record;
use super::shares::{Shares, arm_outcomes, experiment_arms};
use super::spin::Signal;
use crate::ledger::closed::{ArmBox, ClosedReport};
pub use crate::router::brief::{ART, known_category as known};
use crate::router::settle::Outcome;
pub use crate::router::tier::Tier;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::DocumentMut;

/// Window of the closed-box report dispatch's arm and cell draws read.
pub const ARM_WINDOW: &str = "30d";
/// Categories that start on Opus: architecture-class work, and unknown-cause debugging that names no repro.
pub const OPUS: [&str; 5] = ["debugging-unknown-cause", "netcode-determinism", "performance", "planning", "review"];
pub const UNKNOWN_CAUSE: &str = "debugging-unknown-cause";
/// The `Scope:` words of a plan that spans modules, which plans at Opus high; narrower plans run at medium.
pub const SPANNING: [&str; 2] = ["cross-module", "cross-repo"];
/// The auto-compact windows a local Claude box on no tier or effort arm launches with, one equal seeded
/// share each at the start, stamped `Arm: context-NNNk` (docs/rationale/context-economics.md).
pub const CONTEXT_WINDOWS: [u64; 4] = [200_000, 300_000, 400_000, 500_000];

pub fn evidenced() -> Vec<&'static str> {
    crate::router::brief::CATEGORIES.to_vec()
}

/// The effort an Opus box runs at, compared as an arm on its own `Arm:` stamp.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Effort {
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    pub const ALL: [Self; 4] = [Self::Medium, Self::High, Self::Xhigh, Self::Max];

    /// The `--effort` word Claude Code takes.
    pub fn name(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// The `Arm:` stamp a box on this effort's arm carries, which the ledger records.
    pub fn arm(self) -> &'static str {
        match self {
            Self::Medium => "effort-medium",
            Self::High => "effort-high",
            Self::Xhigh => "effort-xhigh",
            Self::Max => "effort-max",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effort| effort.name() == text || effort.arm() == text)
    }
}

/// What the ledger's closed-box report holds for one category: the boxes each arm stamp started and
/// the boxes each model-effort cell started.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    /// Every arm's boxes in the category, keyed by its `Arm:` stamp, which the arm draws read.
    pub outcomes: BTreeMap<String, Vec<Outcome>>,
    pub cell_boxes: BTreeMap<String, Vec<Outcome>>,
}

impl Evidence {
    /// Adds every arm's and every cell's boxes in this category, whose first run carried that stamp in
    /// the ledger's closed-box report.
    pub fn priced(self, report: &ClosedReport, category: &str) -> Self {
        let outcomes = arm_outcomes(report, category);
        let cell_boxes = cells::outcomes(report, category);
        Self { outcomes, cell_boxes }
    }

    /// The shares of the category's next boxes among the arms of its `family` experiment.
    pub fn shares(&self, category: &str, family: &'static str) -> Shares {
        Shares::of(category, family, &experiment_arms(family), &self.outcomes)
    }
}

/// The tier one box launches on, the effort it overrides the tier's with, the arm it records and
/// the earlier launch it follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub tier: Tier,
    pub category: String,
    pub effort: Option<Effort>,
    pub effort_arm: bool,
    pub follows: Option<String>,
    /// The spin signal that climbed this launch off its box's last rung, which the launch record keeps.
    pub signal: Option<Signal>,
    pub why: String,
    pub cell: Option<Cell>,
    pub cell_role: Option<&'static str>,
}

impl Route {
    /// The `Arm:` stamp the brief carries: the effort arm's, when the box runs on one.
    pub fn stamp(&self) -> Option<&'static str> {
        self.effort.filter(|_| self.effort_arm).map(Effort::arm)
    }

    pub fn model_effort(&self, cfg: &DocumentMut) -> Result<(String, String), String> {
        let (model, effort) = self.tier.model_effort(cfg)?;
        let effort = match crate::fleet::registry::efforts::admitted(Some(&model), self.effort.and_then(|effort| crate::fleet::effort::Effort::parse(effort.name()).ok())) {
            Some(admitted) => admitted.label().to_owned(),
            None => self.effort.map_or(effort, |effort| effort.name().to_owned()),
        };
        Ok((model, effort))
    }

    pub fn invocation(&self, cfg: &DocumentMut) -> Result<Vec<String>, String> {
        self.model_effort(cfg).map(|(model, effort)| vec!["--model".into(), model, "--effort".into(), effort])
    }
}

/// Dispatch's earlier launches: each item's rungs and sessions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    tried: BTreeMap<String, Vec<(Rung, String)>>,
}

impl History {
    pub fn new(records: &[Record]) -> Self {
        let mut history = Self::default();
        let mut started: Vec<&Record> = records.iter().filter(|record| record.launched).collect();
        started.sort_by(|a, b| a.at.total_cmp(&b.at));
        for record in started {
            if let Some(rung) = Rung::recorded(record) {
                history.tried.entry(record.item.clone()).or_default().push((rung, record.name.clone()));
            }
        }
        history
    }

    pub fn launched(&mut self, item: &str, name: &str, route: &Route) {
        self.tried.entry(item.to_owned()).or_default().push((Rung::of(route.tier, route.effort), name.to_owned()));
    }

    /// The seed of `item`'s next draw in the `family` experiment; a relaunch draws again.
    pub(super) fn seed(&self, family: &str, item: &str) -> String {
        format!("{family}:{item}#{}", self.tried.get(item).map_or(0, Vec::len))
    }

    fn tried_on(&self, item: &str, on: Tier) -> Option<&str> {
        self.tried.get(item)?.iter().rev().find(|(rung, _)| rung.tier == on).map(|(_, name)| name.as_str())
    }

    /// The launches `item` has already spent on Opus xhigh.
    fn xhigh_attempts(&self, item: &str) -> usize {
        self.tried.get(item).map_or(0, |tried| tried.iter().filter(|(rung, _)| rung.effort == Some(Effort::Xhigh)).count())
    }
}

fn art(category: &str, facet: &dyn Fn(&str) -> String) -> bool {
    let judged = facet("Spec").eq_ignore_ascii_case("judged");
    let pictured = facet("Surface").eq_ignore_ascii_case("assets") || facet("Verify").eq_ignore_ascii_case("visual");
    ART.contains(&category) || judged && pictured
}

/// One rung of the ladder a box climbs when its worker gets nowhere on its own rung
/// (docs/rationale/nearest-routing.md section 5); only Opus rungs carry an effort.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Rung {
    pub tier: Tier,
    pub effort: Option<Effort>,
}

/// The Opus xhigh launches a box gets before its climb reaches Opus max.
pub const XHIGH_ATTEMPTS: usize = 2;

impl Rung {
    pub const LADDER: [Self; 6] = [
        Self { tier: Tier::Haiku, effort: None },
        Self { tier: Tier::Sonnet, effort: None },
        Self { tier: Tier::Opus, effort: Some(Effort::Medium) },
        Self { tier: Tier::Opus, effort: Some(Effort::High) },
        Self { tier: Tier::Opus, effort: Some(Effort::Xhigh) },
        Self { tier: Tier::Opus, effort: Some(Effort::Max) },
    ];

    /// Rungs a box may start on; xhigh is reached only by escalation.
    pub const STARTS: [Self; 4] = [Self::HAIKU, Self::SONNET, Self::OPUS_MEDIUM, Self::OPUS_HIGH];
    pub const SONNET: Self = Self { tier: Tier::Sonnet, effort: None };
    const HAIKU: Self = Self { tier: Tier::Haiku, effort: None };
    const OPUS_MEDIUM: Self = Self { tier: Tier::Opus, effort: Some(Effort::Medium) };
    const OPUS_HIGH: Self = Self { tier: Tier::Opus, effort: Some(Effort::High) };
    const OPUS_XHIGH: Self = Self { tier: Tier::Opus, effort: Some(Effort::Xhigh) };

    /// The rung of a ledger tier key such as `claude:sonnet-high`.
    pub fn of_key(key: &str) -> Option<Self> {
        Some(match key {
            "claude:haiku" => Self::HAIKU,
            "claude:sonnet-high" => Self::SONNET,
            "claude:opus-medium" => Self::OPUS_MEDIUM,
            "claude:opus-high" => Self::OPUS_HIGH,
            "claude:opus-xhigh" => Self::OPUS_XHIGH,
            _ => return None,
        })
    }

    /// The next rung up the ladder a box climbs on worker-caused failure, stopping at xhigh.
    pub fn up(self) -> Option<Self> {
        self.above().filter(|rung| rung.effort != Some(Effort::Max))
    }

    pub fn label(self) -> String {
        self.effort.map_or_else(|| self.tier.family().to_owned(), |effort| format!("{}-{}", self.tier.family(), effort.name()))
    }

    /// The rung `tier` runs on at `effort`; Opus with no override runs at its medium default.
    pub fn of(tier: Tier, effort: Option<Effort>) -> Self {
        Self { tier, effort: (tier == Tier::Opus).then(|| effort.unwrap_or(Effort::Medium)) }
    }

    /// The rung a dispatch record launched on, when it names its tier.
    pub fn recorded(record: &Record) -> Option<Self> {
        let tier = record.tier.as_deref().and_then(Tier::parse)?;
        Some(Self::of(tier, record.effort.as_deref().and_then(Effort::parse)))
    }

    /// The next rung up, or none from the top.
    pub fn above(self) -> Option<Self> {
        Self::LADDER.iter().position(|rung| *rung == self).and_then(|at| Self::LADDER.get(at + 1)).copied()
    }

    /// The rung the next launch climbs to after `attempts` launches have failed here: xhigh retries
    /// once, then every other rung climbs one up.
    pub fn climbed(self, attempts: usize) -> Option<Self> {
        if self.effort == Some(Effort::Xhigh) && attempts < XHIGH_ATTEMPTS { Some(self) } else { self.above() }
    }

    pub fn name(self) -> String {
        self.effort.map_or_else(|| self.tier.name().to_owned(), |effort| format!("{} at {} effort", self.tier.name(), effort.name()))
    }
}

/// The route of `issue`'s next launch: one rung above its last worker's when that worker got nowhere on its rung,
/// else the route its category and arms give.
pub fn route(issue: &Issue, history: &History, ledger: &BTreeMap<String, Evidence>) -> Result<Route, String> {
    let facet = |key: &str| facets::facet(issue, key);
    let Some(climb) = &issue.stop.climb else {
        let route = route_for(&issue.reference().to_string(), &facet, history, ledger)?;
        let follows = route.follows.clone().or_else(|| issue.stop.relay.as_ref().map(|relay| format!("{} (Stop: host {})", relay.session, relay.need)));
        return Ok(Route { follows, ..route });
    };
    let category = facet("Category").to_lowercase();
    known(&category)?;
    let attempts = history.xhigh_attempts(&issue.reference().to_string());
    let rung = climb.rung.climbed(attempts).unwrap_or(climb.rung);
    let why = if rung == climb.rung {
        format!("retries {}'s {} at {} before Opus max", climb.session, climb.why, rung.name())
    } else {
        format!("escalates {}'s {} from {} to {}", climb.session, climb.why, climb.rung.name(), rung.name())
    };
    Ok(Route { tier: rung.tier, category, effort: rung.effort, effort_arm: false, cell: None, cell_role: None, follows: Some(format!("{} ({})", climb.session, climb.rung.name())), signal: Some(climb.signal), why })
}

pub fn route_for(item: &str, facet: &dyn Fn(&str) -> String, history: &History, ledger: &BTreeMap<String, Evidence>) -> Result<Route, String> {
    let category = facet("Category").to_lowercase();
    known(&category)?;
    let evidence = ledger.get(&category).cloned().unwrap_or_default();
    let route = tiered(item, &category, facet, history);
    if route.follows.is_some() {
        return Ok(route);
    }
    let seed = Cell::seed(route.tier, route.effort);
    let seed_text = history.seed("cell", item);
    let (cell, role, drawn) = cells::draw(cells::Shape { category: &category, scope: &facet("Scope").to_lowercase(), verify: &facet("Verify").to_lowercase() }, seed, &seed_text, &evidence.cell_boxes);
    Ok(Route {
        tier: cell.tier(),
        effort: Some(cell.effort()),
        effort_arm: false,
        cell: Some(cell),
        cell_role: Some(role.name()),
        why: format!("{}, {drawn}", route.why),
        ..route
    })
}

impl Cell {
    pub fn tier(self) -> Tier {
        match self.model {
            "haiku" => Tier::Haiku,
            "sonnet" => Tier::Sonnet,
            _ => Tier::Opus,
        }
    }

    pub fn effort(self) -> Effort {
        Effort::parse(self.effort).unwrap_or(Effort::High)
    }

    pub fn seed(tier: Tier, effort: Option<Effort>) -> Self {
        let effort = effort.unwrap_or(match tier {
            Tier::Haiku | Tier::Sonnet => Effort::High,
            Tier::Opus => Effort::Medium,
        });
        let model = match tier {
            Tier::Haiku => "haiku",
            Tier::Sonnet => "sonnet",
            Tier::Opus => "opus",
        };
        Self { model, effort: effort.name() }
    }
}

pub fn seed_cell(category: &str) -> Cell {
    let facet = |key: &str| facets::default(key).to_owned();
    let route = tiered("north#939", category, &facet, &History::default());
    Cell::seed(route.tier, route.effort)
}

fn tiered(item: &str, category: &str, facet: &dyn Fn(&str) -> String, history: &History) -> Route {
    let on = |tier: Tier, why: String| Route { tier, category: category.to_owned(), effort: None, effort_arm: false, cell: None, cell_role: None, follows: None, signal: None, why };
    if art(category, facet) {
        return on(Tier::Sonnet, "art goes to sonnet-high".into());
    }
    if category == "mechanical" {
        return match history.tried_on(item, Tier::Haiku) {
            Some(name) => Route { follows: Some(format!("{name} (worker-haiku)")), ..on(Tier::Sonnet, format!("retries the Haiku miss {name} on sonnet-high")) },
            None => on(Tier::Haiku, "mechanical work goes to worker-haiku".into()),
        };
    }
    if category == UNKNOWN_CAUSE {
        let repro = facet("Repro");
        return match history.tried_on(item, Tier::Sonnet) {
            Some(name) if !repro.is_empty() => Route { follows: Some(format!("{name} (worker-sonnet)")), effort: Some(Effort::Medium), ..on(Tier::Opus, format!("escalates the Sonnet try {name} on an unknown-cause bug with repro {repro} to worker at medium effort, its one escalation")) },
            None if !repro.is_empty() => on(Tier::Sonnet, format!("an unknown-cause bug naming repro {repro} starts on sonnet-high")),
            _ => Route { effort: Some(Effort::Medium), ..on(Tier::Opus, "an unknown-cause bug naming no failing test or repro command starts on worker at medium effort".into()) },
        };
    }
    if category == "planning" {
        let scope = facet("Scope").to_lowercase();
        let (effort, why) = if SPANNING.contains(&scope.as_str()) { (Effort::High, "spans modules, so worker at high effort") } else { (Effort::Medium, "stays in one module, so worker at medium effort") };
        return Route { effort: Some(effort), ..on(Tier::Opus, format!("planning at Scope: {scope} {why}")) };
    }
    if OPUS.contains(&category) {
        return on(Tier::Opus, format!("{category} starts on worker"));
    }
    on(Tier::Sonnet, format!("{category} starts on sonnet-high"))
}

/// The context arm this item's next launch draws from the category's context shares, and the words
/// naming the share it drew from.
pub fn context_arm(item: &str, category: &str, history: &History, evidence: &Evidence) -> (String, String) {
    let shares = evidence.shares(category, "context");
    let (arm, drawn) = shares.draw(&history.seed("context", item));
    (arm.to_owned(), drawn)
}

/// The pack arm this item's next launch draws from the category's pack shares, and the words naming
/// the share it drew from.
pub fn pack_arm(item: &str, category: &str, history: &History, evidence: &Evidence) -> (String, String) {
    let shares = evidence.shares(category, "pack");
    let (arm, drawn) = shares.draw(&history.seed("pack", item));
    (arm.to_owned(), drawn)
}

/// Each context arm's boxes, closed boxes, kept boxes and cost per kept box among the fleet's arm
/// boxes, each item counted once.
pub fn context_figures(arm_boxes: &[ArmBox]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let unique: Vec<&ArmBox> = arm_boxes.iter().filter(|armed| seen.insert(armed.item.as_str())).collect();
    CONTEXT_WINDOWS
        .map(crate::fleet::compaction::arm)
        .into_iter()
        .map(|arm| {
            let boxes: Vec<_> = unique.iter().filter(|armed| armed.arm == arm).collect();
            let closed = boxes.iter().filter(|armed| armed.closed).count();
            let kept = boxes.iter().filter(|armed| armed.closed && !armed.reworked).count();
            let usd: f64 = boxes.iter().map(|armed| armed.usd).sum();
            let cost = if kept == 0 { "no kept box".to_owned() } else { format!("${:.2} per kept box", usd / kept as f64) };
            format!("context arm {arm}: {} boxes, {closed} closed, {kept} kept, {cost} over {ARM_WINDOW}", boxes.len())
        })
        .collect()
}
