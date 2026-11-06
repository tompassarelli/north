//! Which worker tier a dispatched box starts on: Sonnet for art, Haiku for mechanical work with its
//! retry on Sonnet, the Sonnet and Opus arms in turn on the armed categories until the ledger holds
//! 15 runs and 15 closed boxes of each, then the arm with the lower cost per closed box, and Opus for
//! the rest. An Opus box on no tier arm in a category with 15 Opus runs alternates the medium and
//! high effort arms the same way until the tune tick or the ledger settles the cheaper one. A planner
//! or reviewer run takes Opus high, and xhigh on a seeded half of its launches, until the ledger
//! settles the cheaper of the two.
use super::facets;
use super::issues::Issue;
use super::record::Record;
use crate::ledger::closed::ClosedReport;
use crate::router::policy::ARM_RUNS;
pub use crate::router::tier::Tier;
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

/// Window of the ledger's arm boxes whose cost per closed box decides an armed category; it spans the
/// arms' whole trial, because a week held 14 closed boxes per arm at most (north#242, 2026-11-05).
pub const ARM_WINDOW: &str = "30d";
/// Categories whose Sonnet and Opus arms dispatch compares.
pub const ARMED: [&str; 6] = ["feature", "tooling", "native-check", "balance-tuning", "bug-known-cause", "performance"];
/// Categories of planner and reviewer runs, whose effort arms are high and xhigh.
pub const PLANNED: [&str; 2] = ["planning", "review"];
/// The share of planner and reviewer launches drawn onto the xhigh arm, in percent.
pub const XHIGH_PERCENT: u64 = 50;
const ART: [&str; 3] = ["anim", "animation", "art"];

/// Categories whose ledger evidence dispatch and the tune tick read: every category but mechanical
/// work, which never runs on Opus.
pub fn evidenced() -> Vec<&'static str> {
    crate::router::brief::CATEGORIES.into_iter().filter(|category| *category != "mechanical").collect()
}

/// The effort an Opus box runs at, compared as an arm on its own `Arm:` stamp.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Effort {
    Medium,
    High,
    Xhigh,
}

impl Effort {
    pub const ARMS: [Self; 2] = [Self::Medium, Self::High];
    pub const ALL: [Self; 3] = [Self::Medium, Self::High, Self::Xhigh];

    /// The `--effort` word Claude Code takes, which is also the tune cell's value.
    pub fn name(self) -> &'static str {
        match self {
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }

    /// The `Arm:` stamp a box on this effort's arm carries, which the ledger records.
    pub fn arm(self) -> &'static str {
        match self {
            Self::Medium => "effort-medium",
            Self::High => "effort-high",
            Self::Xhigh => "effort-xhigh",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effort| effort.name() == text || effort.arm() == text)
    }

    fn parse_arm(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effort| effort.arm() == text)
    }

    fn other(self) -> Self {
        if self == Self::Medium { Self::High } else { Self::Medium }
    }
}

/// One arm comparison's evidence in a category: the runs stamped with each arm, each arm's closed
/// boxes, and its cost per kept box once the closed-box report is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Trial<K: Ord> {
    pub runs: BTreeMap<K, usize>,
    pub closed: BTreeMap<K, usize>,
    pub usd_per_kept: Option<BTreeMap<K, f64>>,
}

impl<K: Ord> Default for Trial<K> {
    fn default() -> Self {
        Self { runs: BTreeMap::new(), closed: BTreeMap::new(), usd_per_kept: None }
    }
}

impl<K: Ord + Copy> Trial<K> {
    fn counted(arms: &[K], stamp: impl Fn(K) -> &'static str, stamped: &BTreeMap<String, usize>) -> BTreeMap<K, usize> {
        arms.iter().map(|arm| (*arm, stamped.get(stamp(*arm)).copied().unwrap_or(0))).collect()
    }

    /// The spend of each arm's boxes in `category`, closed or not, over its closed boxes not
    /// reworked; an arm with no closed box has no cost.
    fn priced(self, report: &ClosedReport, category: &str, arm_of: impl Fn(&str) -> Option<K>) -> Self {
        let mut sums: BTreeMap<K, (f64, usize, usize)> = BTreeMap::new();
        for armed in report.arms.iter().filter(|armed| armed.category == category) {
            if let Some(arm) = arm_of(&armed.arm) {
                let sum = sums.entry(arm).or_default();
                *sum = (sum.0 + armed.usd, sum.1 + usize::from(armed.closed), sum.2 + usize::from(armed.closed && !armed.reworked));
            }
        }
        let closed = sums.iter().map(|(arm, (_, closed, _))| (*arm, *closed)).collect();
        let per_kept = |(usd, _, kept): (f64, usize, usize)| if kept == 0 { f64::INFINITY } else { usd / kept as f64 };
        let usd_per_kept = sums.into_iter().filter(|(_, (_, closed, _))| *closed > 0).map(|(arm, sum)| (arm, per_kept(sum))).collect();
        Self { closed, usd_per_kept: Some(usd_per_kept), ..self }
    }

    fn runs(&self, arm: K) -> usize {
        self.runs.get(&arm).copied().unwrap_or(0)
    }

    fn closed(&self, arm: K) -> usize {
        self.closed.get(&arm).copied().unwrap_or(0)
    }

    fn cost(&self, arm: K) -> Option<f64> {
        self.usd_per_kept.as_ref()?.get(&arm).copied()
    }

    /// The arm with the lower cost per kept box, `first` on a tie, once both arms hold `ARM_RUNS`
    /// stamped runs and `ARM_RUNS` closed boxes and both are priced.
    fn settled(&self, first: K, second: K) -> Option<K> {
        let full = |arm: K| self.runs(arm) >= ARM_RUNS && self.closed(arm) >= ARM_RUNS;
        let cheaper = if self.cost(second)? < self.cost(first)? { second } else { first };
        (full(first) && full(second)).then_some(cheaper)
    }

    fn figures(&self, arm: K, name: &str) -> String {
        let cost = match self.cost(arm) {
            None => "no closed box".to_owned(),
            Some(usd) if usd.is_infinite() => "every closed box reworked".to_owned(),
            Some(usd) => format!("${usd:.2} per kept box"),
        };
        format!("{name} {} of {ARM_RUNS} arm runs, {} of {ARM_RUNS} closed arm boxes, {cost}", self.runs(arm), self.closed(arm))
    }
}

/// What the ledger holds for one category: each tier's success rate and runs there from
/// `--recommend`, the runs stamped with each tier's arm, and each arm's closed boxes and cost per
/// closed box from `north ledger summary`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    pub tiers: BTreeMap<String, (f64, usize)>,
    pub arm_runs: BTreeMap<Tier, usize>,
    pub usd_per_closed: Option<BTreeMap<Tier, f64>>,
    pub arm_closed: BTreeMap<Tier, usize>,
    pub tuned: Option<Tier>,
    pub efforts: Trial<Effort>,
    pub tuned_effort: Option<Effort>,
}

impl Evidence {
    /// Reads `north worker-ledger --recommend 'Category: NAME'`; a coarser cell than the category counts
    /// as no evidence.
    pub fn parse(printed: &str, category: &str) -> Self {
        let Some((_, cell)) = printed.lines().find_map(|line| line.split_once("  cell [")) else { return Self::default() };
        if !cell.to_lowercase().starts_with(&format!("category: {category}]")) {
            return Self::default();
        }
        let tiers = printed
            .lines()
            .filter(|line| line.starts_with("  "))
            .filter_map(|line| {
                let (label, rest) = line.trim().split_once('%')?;
                let (label, rate) = label.rsplit_once(' ')?;
                Some((label.to_owned(), (rate.parse().ok()?, rest.trim().strip_prefix("of ")?.split_whitespace().next()?.parse().ok()?)))
            })
            .collect();
        Self { tiers, ..Self::default() }
    }

    /// Adds each arm's closed boxes and cost per closed box in this category from the boxes whose
    /// first run carried its `Arm:` line in the ledger's closed-box report: the spend of all its boxes,
    /// closed or not, over the closed boxes not reworked. A tier's unstamped boxes carry the earlier
    /// routing's selection and are left out, as the arm run count leaves them out; an arm with no
    /// closed box has no cost.
    pub fn priced(self, report: &ClosedReport, category: &str) -> Self {
        let mut sums: BTreeMap<Tier, (f64, usize, usize)> = BTreeMap::new();
        for armed in report.arms.iter().filter(|armed| armed.category == category) {
            if let Some(tier) = [Tier::Sonnet, Tier::Opus].into_iter().find(|tier| tier.name() == armed.arm) {
                let sum = sums.entry(tier).or_default();
                *sum = (sum.0 + armed.usd, sum.1 + usize::from(armed.closed), sum.2 + usize::from(armed.closed && !armed.reworked));
            }
        }
        let arm_closed = sums.iter().map(|(tier, (_, closed, _))| (*tier, *closed)).collect();
        let per_kept = |(usd, _, kept): (f64, usize, usize)| if kept == 0 { f64::INFINITY } else { usd / kept as f64 };
        let usd_per_closed = sums.into_iter().filter(|(_, (_, closed, _))| *closed > 0).map(|(tier, sum)| (tier, per_kept(sum))).collect();
        let efforts = self.efforts.clone().priced(report, category, Effort::parse_arm);
        Self { usd_per_closed: Some(usd_per_closed), arm_closed, efforts, ..self }
    }

    pub fn tuned(self, routes: &BTreeMap<String, Tier>, category: &str) -> Self {
        Self { tuned: routes.get(category).copied(), ..self }
    }

    pub fn tuned_effort(self, efforts: &BTreeMap<String, Effort>, category: &str) -> Self {
        Self { tuned_effort: efforts.get(category).copied(), ..self }
    }

    /// Adds the runs the ledger holds stamped with each tier's arm and each effort's arm in this
    /// category.
    pub fn counted(self, stamped: &BTreeMap<String, usize>) -> Self {
        let arm_runs = Trial::counted(&[Tier::Sonnet, Tier::Opus], Tier::name, stamped);
        let efforts = Trial { runs: Trial::counted(&Effort::ALL, Effort::arm, stamped), ..self.efforts.clone() };
        Self { arm_runs, efforts, ..self }
    }

    /// The effort the category's Opus boxes settle on: medium on a tie.
    pub fn settled_effort(&self) -> Option<Effort> {
        self.efforts.settled(Effort::Medium, Effort::High)
    }

    /// The effort a planner or reviewer run settles on: high on a tie.
    pub fn settled_planned_effort(&self) -> Option<Effort> {
        self.efforts.settled(Effort::High, Effort::Xhigh)
    }

    /// Both effort arms' stamped runs behind the decision.
    pub fn effort_runs(&self) -> usize {
        Effort::ARMS.into_iter().map(|effort| self.efforts.runs(effort)).sum()
    }

    /// Both effort arms' figures, as a launch reason or a tune cell's claim shows them.
    pub fn effort_figures(&self) -> String {
        Effort::ARMS.map(|effort| self.efforts.figures(effort, effort.arm())).join("; ")
    }

    fn runs(&self, tier: Tier) -> usize {
        self.arm_runs.get(&tier).copied().unwrap_or(0)
    }

    fn all_runs(&self, tier: Tier) -> usize {
        self.tiers.get(tier.key()).map_or(0, |(_, runs)| *runs)
    }

    fn rate(&self, tier: Tier) -> f64 {
        self.tiers.get(tier.key()).map_or(0.0, |(rate, _)| *rate)
    }

    fn cost(&self, tier: Tier) -> Option<f64> {
        self.usd_per_closed.as_ref()?.get(&tier).copied()
    }

    /// The arm with the lower cost per closed box, Sonnet on a tie. None until the closed-box
    /// report is read and both arms have a closed box in it, so an arm the window forgot keeps
    /// running rather than losing by default.
    pub(super) fn decided(&self) -> Option<Tier> {
        Some(if self.cost(Tier::Opus)? < self.cost(Tier::Sonnet)? { Tier::Opus } else { Tier::Sonnet })
    }

    fn closed(&self, tier: Tier) -> usize {
        self.arm_closed.get(&tier).copied().unwrap_or(0)
    }

    /// The arm the category routes to once both arms hold `ARM_RUNS` arm-stamped runs and `ARM_RUNS`
    /// closed arm boxes; None while the arms still alternate.
    pub fn settled(&self) -> Option<Tier> {
        let full = |tier: Tier| self.runs(tier) >= ARM_RUNS && self.closed(tier) >= ARM_RUNS;
        self.decided().filter(|_| full(Tier::Sonnet) && full(Tier::Opus))
    }

    /// Whether both arms hold `ARM_RUNS` arm-stamped runs, the point where a category stops alternating.
    pub fn arms_full(&self) -> bool {
        self.runs(Tier::Sonnet) >= ARM_RUNS && self.runs(Tier::Opus) >= ARM_RUNS
    }

    /// The first arm with no priced closed box, which keeps `settled` from deciding.
    pub fn unpriced_arm(&self) -> Option<Tier> {
        [Tier::Sonnet, Tier::Opus].into_iter().find(|tier| self.cost(*tier).is_none())
    }

    /// Both arms' stamped runs behind the decision.
    pub fn arm_runs(&self) -> usize {
        self.runs(Tier::Sonnet) + self.runs(Tier::Opus)
    }

    pub fn figures(&self, tier: Tier) -> String {
        let cost = match self.cost(tier) {
            None => "no closed box".to_owned(),
            Some(usd) if usd.is_infinite() => "every closed box reworked".to_owned(),
            Some(usd) => format!("${usd:.2} per closed box"),
        };
        format!("{} {:.0}% of {}, {} of {ARM_RUNS} arm runs, {} of {ARM_RUNS} closed arm boxes, {cost}", tier.name(), self.rate(tier), self.all_runs(tier), self.runs(tier), self.closed(tier))
    }
}

/// The tier one box launches on, the effort it overrides the tier's with, the arm it records and
/// the earlier launch it follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub tier: Tier,
    pub category: String,
    pub arm: bool,
    pub effort: Option<Effort>,
    pub effort_arm: bool,
    pub follows: Option<String>,
    pub why: String,
}

impl Route {
    /// The `Arm:` stamp the brief carries: the tier's arm, else the effort's.
    pub fn stamp(&self) -> Option<&'static str> {
        if self.arm { Some(self.tier.name()) } else { self.effort_stamp() }
    }

    /// The effort arm's stamp, when the box runs on one.
    pub fn effort_stamp(&self) -> Option<&'static str> {
        self.effort.filter(|_| self.effort_arm).map(Effort::arm)
    }

    pub fn model_effort(&self, cfg: &DocumentMut) -> Result<(String, String), String> {
        let (model, effort) = self.tier.model_effort(cfg)?;
        Ok((model, self.effort.map_or(effort, |effort| effort.name().to_owned())))
    }

    pub fn invocation(&self, cfg: &DocumentMut) -> Result<Vec<String>, String> {
        self.model_effort(cfg).map(|(model, effort)| vec!["--model".into(), model, "--effort".into(), effort])
    }
}

/// Dispatch's earlier launches: each item's tiers and sessions, and each category's last tier arm
/// and last effort arm.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    tried: BTreeMap<String, Vec<(Tier, String)>>,
    last_arm: BTreeMap<String, Tier>,
    last_effort: BTreeMap<String, Effort>,
}

impl History {
    pub fn new(records: &[Record]) -> Self {
        let mut history = Self::default();
        let mut started: Vec<&Record> = records.iter().filter(|record| record.launched).collect();
        started.sort_by(|a, b| a.at.total_cmp(&b.at));
        for record in started {
            let Some(tier) = record.tier.as_deref().and_then(Tier::parse) else { continue };
            let effort = record.effort_arm.as_deref().and_then(Effort::parse);
            let category = record.category.clone().unwrap_or_default();
            let route = Route { tier, category, arm: record.arm, effort, effort_arm: effort.is_some(), follows: None, why: String::new() };
            history.launched(&record.item, &record.name, &route);
        }
        history
    }

    pub fn launched(&mut self, item: &str, name: &str, route: &Route) {
        self.tried.entry(item.to_owned()).or_default().push((route.tier, name.to_owned()));
        self.chose(route);
    }

    /// Takes a route placed in this run, so the next box in its category gets the other arm.
    pub fn chose(&mut self, route: &Route) {
        if route.arm {
            self.last_arm.insert(route.category.clone(), route.tier);
        }
        if let Some(effort) = route.effort.filter(|_| route.effort_arm) {
            self.last_effort.insert(route.category.clone(), effort);
        }
    }

    fn launches(&self, item: &str) -> usize {
        self.tried.get(item).map_or(0, Vec::len)
    }

    fn haiku_try(&self, item: &str) -> Option<&str> {
        self.tried.get(item)?.iter().rev().find(|(tier, _)| *tier == Tier::Haiku).map(|(_, name)| name.as_str())
    }
}

fn other_arm(tier: Tier) -> Tier {
    if tier == Tier::Sonnet { Tier::Opus } else { Tier::Sonnet }
}

fn art(category: &str, facet: &dyn Fn(&str) -> String) -> bool {
    let judged = facet("Spec").eq_ignore_ascii_case("judged");
    let pictured = facet("Surface").eq_ignore_ascii_case("assets") || facet("Verify").eq_ignore_ascii_case("visual");
    ART.contains(&category) || judged && pictured
}

pub fn route(issue: &Issue, history: &History, ledger: &BTreeMap<String, Evidence>) -> Route {
    route_for(&issue.reference().to_string(), &|key| facets::facet(issue, key), history, ledger)
}

pub fn route_for(item: &str, facet: &dyn Fn(&str) -> String, history: &History, ledger: &BTreeMap<String, Evidence>) -> Route {
    let category = facet("Category").to_lowercase();
    let evidence = ledger.get(&category).cloned().unwrap_or_default();
    let route = tiered(item, &category, facet, history, &evidence);
    if PLANNED.contains(&category.as_str()) { planned(route, item, history, &evidence) } else { efforted(route, history, &evidence) }
}

fn tiered(item: &str, category: &str, facet: &dyn Fn(&str) -> String, history: &History, evidence: &Evidence) -> Route {
    let on = |tier: Tier, why: String| Route { tier, category: category.to_owned(), arm: false, effort: None, effort_arm: false, follows: None, why };
    if art(category, facet) {
        return on(Tier::Sonnet, "art goes to sonnet-high".into());
    }
    if category == "mechanical" {
        return match history.haiku_try(item) {
            Some(name) => Route { follows: Some(format!("{name} (worker-haiku)")), ..on(Tier::Sonnet, format!("retries the Haiku miss {name} on sonnet-high")) },
            None => on(Tier::Haiku, "mechanical work goes to worker-haiku".into()),
        };
    }
    if !ARMED.contains(&category) {
        return on(Tier::Opus, format!("{category} stays on worker"));
    }
    if let Some(tier) = evidence.tuned {
        return on(tier, format!("{category} goes to {} by north tune's route cell, written within {}h", tier.name(), super::tuned::FRESH_HOURS));
    }
    if let Some(tier) = evidence.settled() {
        return on(tier, format!("{category} goes to {} by the ledger's cost per closed box ({}; {})", tier.name(), evidence.figures(Tier::Sonnet), evidence.figures(Tier::Opus)));
    }
    let tier = history.last_arm.get(category).map_or(Tier::Sonnet, |last| other_arm(*last));
    Route { arm: true, ..on(tier, format!("{category} arm {} ({}; {}; {ARM_RUNS} each to decide)", tier.name(), evidence.figures(Tier::Sonnet), evidence.figures(Tier::Opus))) }
}

/// An Opus box on no tier arm takes north tune's effort cell, else the ledger's settled effort, else
/// the next effort arm once the category holds `ARM_RUNS` Opus runs; every other box keeps its tier's
/// effort.
fn efforted(route: Route, history: &History, evidence: &Evidence) -> Route {
    if route.tier != Tier::Opus || route.arm {
        return route;
    }
    let category = &route.category;
    let (effort, arm, why) = if let Some(effort) = evidence.tuned_effort {
        (effort, false, format!("at {} effort by north tune's effort cell, written within {}h", effort.name(), super::tuned::FRESH_HOURS))
    } else if let Some(effort) = evidence.settled_effort() {
        (effort, false, format!("at {} effort by the ledger's cost per kept box ({})", effort.name(), evidence.effort_figures()))
    } else if evidence.all_runs(Tier::Opus) >= ARM_RUNS {
        let effort = history.last_effort.get(category).map_or(Effort::Medium, |last| last.other());
        (effort, true, format!("on effort arm {} ({}; {ARM_RUNS} each to decide)", effort.arm(), evidence.effort_figures()))
    } else {
        return route;
    };
    Route { effort: Some(effort), effort_arm: arm, why: format!("{}, {why}", route.why), ..route }
}

/// A planner or reviewer run's effort: the ledger's settled arm, else xhigh when the seeded draw for
/// this item's next launch falls in `XHIGH_PERCENT`, else high.
fn planned(route: Route, item: &str, history: &History, evidence: &Evidence) -> Route {
    let figures = || [Effort::High, Effort::Xhigh].map(|effort| evidence.efforts.figures(effort, effort.arm())).join("; ");
    let (effort, arm, why) = if let Some(effort) = evidence.settled_planned_effort() {
        (effort, false, format!("at {} effort by the ledger's cost per kept box ({})", effort.name(), figures()))
    } else {
        let seed = format!("{item}#{}", history.launches(item));
        let effort = if xhigh_drawn(&seed) { Effort::Xhigh } else { Effort::High };
        (effort, true, format!("on effort arm {} by the {XHIGH_PERCENT}% xhigh draw seeded {seed} ({}; {ARM_RUNS} each to decide)", effort.arm(), figures()))
    };
    Route { tier: Tier::Opus, effort: Some(effort), effort_arm: arm, why: format!("{}, {why}", route.why), ..route }
}

/// Whether `seed` draws the xhigh arm: its FNV-1a hash modulo 100 under `XHIGH_PERCENT`, so a launch
/// draws the same arm on every host and rerun.
pub fn xhigh_drawn(seed: &str) -> bool {
    let hash = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    hash % 100 < XHIGH_PERCENT
}
