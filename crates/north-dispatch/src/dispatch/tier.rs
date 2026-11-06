//! Which worker tier a dispatched box starts on: Sonnet for art, Haiku for mechanical work with its
//! retry on Sonnet, the Sonnet or Opus arm on the armed categories until `router::settle` gives one on
//! the evidence (15 runs and 15 closed boxes of each and a bootstrap that puts it ahead), and Opus for
//! the rest. An Opus box on no tier arm in a category with 15 Opus runs takes the medium or high effort
//! arm until the tune tick or the ledger settles the cheaper one. A planner or reviewer run takes the
//! high or xhigh effort arm until the ledger settles the cheaper of the two. A local box on none of
//! those arms takes one auto-compact window of `CONTEXT_WINDOWS`. Every arm is a seeded draw from the
//! shares `Shares` reads off the ledger, the same shares `north climb run --dry-run` prints: equal
//! while the arms hold no boxes, then Thompson sampling on cost per kept box with a floor per arm.
use super::facets;
use super::issues::Issue;
use super::record::Record;
use super::shares::{Shares, arm_outcomes, experiment_arms};
use crate::ledger::closed::{ArmBox, ClosedReport};
use crate::router::settle::{self, Outcome};
use crate::router::policy::ARM_RUNS;
pub use crate::router::tier::Tier;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::DocumentMut;

/// Window of the ledger's arm boxes whose cost per closed box decides an armed category; it spans the
/// arms' whole trial, because a week held 14 closed boxes per arm at most (north#242, 2026-11-05).
pub const ARM_WINDOW: &str = "30d";
/// Categories whose Sonnet and Opus arms dispatch compares.
pub const ARMED: [&str; 6] = ["feature", "tooling", "native-check", "balance-tuning", "bug-known-cause", "performance"];
/// Categories of planner and reviewer runs, whose effort arms are high and xhigh.
pub const PLANNED: [&str; 2] = ["planning", "review"];
/// The auto-compact windows a local Claude box on no tier or effort arm launches with, one equal seeded
/// share each at the start, stamped `Arm: context-NNNk` (docs/rationale/context-economics.md).
pub const CONTEXT_WINDOWS: [u64; 4] = [200_000, 300_000, 400_000, 500_000];
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
}

/// One arm comparison's evidence in a category: the runs stamped with each arm and, once the
/// closed-box report is read, the boxes each arm's first runs started.
#[derive(Clone, Debug, PartialEq)]
pub struct Trial<K: Ord> {
    pub runs: BTreeMap<K, usize>,
    pub boxes: Option<BTreeMap<K, Vec<Outcome>>>,
}

impl<K: Ord> Default for Trial<K> {
    fn default() -> Self {
        Self { runs: BTreeMap::new(), boxes: None }
    }
}

impl<K: Ord + Copy> Trial<K> {
    fn counted(arms: &[K], stamp: impl Fn(K) -> &'static str, stamped: &BTreeMap<String, usize>) -> BTreeMap<K, usize> {
        arms.iter().map(|arm| (*arm, stamped.get(stamp(*arm)).copied().unwrap_or(0))).collect()
    }

    /// Each arm's boxes, closed or not, from one category's `arm_outcomes`.
    fn priced(self, outcomes: &BTreeMap<String, Vec<Outcome>>, arm_of: impl Fn(&str) -> Option<K>) -> Self {
        let boxes = outcomes.iter().filter_map(|(arm, boxes)| Some((arm_of(arm)?, boxes.clone()))).collect();
        Self { boxes: Some(boxes), ..self }
    }

    fn runs(&self, arm: K) -> usize {
        self.runs.get(&arm).copied().unwrap_or(0)
    }

    fn arm_boxes(&self, arm: K) -> &[Outcome] {
        self.boxes.as_ref().and_then(|boxes| boxes.get(&arm)).map_or(&[], Vec::as_slice)
    }

    pub fn closed(&self, arm: K) -> usize {
        settle::closed(self.arm_boxes(arm))
    }

    fn cost(&self, arm: K) -> Option<f64> {
        settle::usd_per_kept(self.arm_boxes(arm))
    }

    /// The arm `settle::settled` gives the comparison, `first` on a tie.
    fn settled(&self, first: K, second: K) -> Option<K> {
        let arm = |arm: K| settle::Arm { runs: self.runs(arm), boxes: self.arm_boxes(arm) };
        settle::settled(arm(first), arm(second)).map(|side| if side == settle::Side::First { first } else { second })
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
/// `--recommend`, and the Sonnet and Opus arms' stamped runs and boxes from `north ledger summary`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evidence {
    pub tiers: BTreeMap<String, (f64, usize)>,
    pub arms: Trial<Tier>,
    pub tuned: Option<Tier>,
    pub efforts: Trial<Effort>,
    pub tuned_effort: Option<Effort>,
    /// Every arm's boxes in the category, keyed by its `Arm:` stamp, which the arm draws read.
    pub outcomes: BTreeMap<String, Vec<Outcome>>,
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

    /// Adds the Sonnet and Opus arms' boxes in this category, those whose first run carried the arm's
    /// `Arm:` line in the ledger's closed-box report. A tier's unstamped boxes carry the earlier
    /// routing's selection and are left out, as the arm run count leaves them out.
    pub fn priced(self, report: &ClosedReport, category: &str) -> Self {
        let outcomes = arm_outcomes(report, category);
        let arms = self.arms.clone().priced(&outcomes, |arm| [Tier::Sonnet, Tier::Opus].into_iter().find(|tier| tier.name() == arm));
        let efforts = self.efforts.clone().priced(&outcomes, Effort::parse_arm);
        Self { arms, efforts, outcomes, ..self }
    }

    /// The shares of the category's next boxes among the arms of its `family` experiment.
    pub fn shares(&self, category: &str, family: &'static str) -> Shares {
        Shares::of(category, family, &experiment_arms(category, family), &self.outcomes)
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
        let arms = Trial { runs: Trial::counted(&[Tier::Sonnet, Tier::Opus], Tier::name, stamped), ..self.arms.clone() };
        let efforts = Trial { runs: Trial::counted(&Effort::ALL, Effort::arm, stamped), ..self.efforts.clone() };
        Self { arms, efforts, ..self }
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
        self.arms.runs(tier)
    }

    fn all_runs(&self, tier: Tier) -> usize {
        self.tiers.get(tier.key()).map_or(0, |(_, runs)| *runs)
    }

    fn rate(&self, tier: Tier) -> f64 {
        self.tiers.get(tier.key()).map_or(0.0, |(rate, _)| *rate)
    }

    fn cost(&self, tier: Tier) -> Option<f64> {
        self.arms.cost(tier)
    }

    fn closed(&self, tier: Tier) -> usize {
        self.arms.closed(tier)
    }

    /// The arm the category routes to once `settle::settled` decides between them, Sonnet on a tie;
    /// None while the arms still draw.
    pub fn settled(&self) -> Option<Tier> {
        self.arms.settled(Tier::Sonnet, Tier::Opus)
    }

    /// Whether both arms hold `ARM_RUNS` arm-stamped runs, the point where a category can stop drawing.
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

/// Dispatch's earlier launches: each item's tiers and sessions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    tried: BTreeMap<String, Vec<(Tier, String)>>,
}

impl History {
    pub fn new(records: &[Record]) -> Self {
        let mut history = Self::default();
        let mut started: Vec<&Record> = records.iter().filter(|record| record.launched).collect();
        started.sort_by(|a, b| a.at.total_cmp(&b.at));
        for record in started {
            if let Some(tier) = record.tier.as_deref().and_then(Tier::parse) {
                history.tried.entry(record.item.clone()).or_default().push((tier, record.name.clone()));
            }
        }
        history
    }

    pub fn launched(&mut self, item: &str, name: &str, route: &Route) {
        self.tried.entry(item.to_owned()).or_default().push((route.tier, name.to_owned()));
    }

    /// The seed of `item`'s next draw in the `family` experiment; a relaunch draws again.
    fn seed(&self, family: &str, item: &str) -> String {
        format!("{family}:{item}#{}", self.tried.get(item).map_or(0, Vec::len))
    }

    fn haiku_try(&self, item: &str) -> Option<&str> {
        self.tried.get(item)?.iter().rev().find(|(tier, _)| *tier == Tier::Haiku).map(|(_, name)| name.as_str())
    }
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
    if PLANNED.contains(&category.as_str()) { planned(route, item, history, &evidence) } else { efforted(route, item, history, &evidence) }
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
    let shares = evidence.shares(category, "tier");
    let (arm, drawn) = shares.draw(&history.seed("tier", item));
    let tier = [Tier::Sonnet, Tier::Opus].into_iter().find(|tier| tier.name() == arm).unwrap_or(Tier::Sonnet);
    Route { arm: true, ..on(tier, format!("{category} arm {arm} {drawn} ({}; {}; {ARM_RUNS} each to decide)", evidence.figures(Tier::Sonnet), evidence.figures(Tier::Opus))) }
}

/// An Opus box on no tier arm takes north tune's effort cell, else the ledger's settled effort, else
/// an effort arm drawn from the category's effort shares once it holds `ARM_RUNS` Opus runs; every
/// other box keeps its tier's effort.
fn efforted(route: Route, item: &str, history: &History, evidence: &Evidence) -> Route {
    if route.tier != Tier::Opus || route.arm {
        return route;
    }
    let (effort, arm, why) = if let Some(effort) = evidence.tuned_effort {
        (effort, false, format!("at {} effort by north tune's effort cell, written within {}h", effort.name(), super::tuned::FRESH_HOURS))
    } else if let Some(effort) = evidence.settled_effort() {
        (effort, false, format!("at {} effort by the ledger's cost per kept box ({})", effort.name(), evidence.effort_figures()))
    } else if evidence.all_runs(Tier::Opus) >= ARM_RUNS {
        let (effort, drawn) = drawn_effort(&route.category, item, history, evidence);
        (effort, true, format!("on effort arm {} {drawn} ({}; {ARM_RUNS} each to decide)", effort.arm(), evidence.effort_figures()))
    } else {
        return route;
    };
    Route { effort: Some(effort), effort_arm: arm, why: format!("{}, {why}", route.why), ..route }
}

/// A planner or reviewer run's effort: the ledger's settled arm, else the high or xhigh arm drawn from
/// the category's effort shares.
fn planned(route: Route, item: &str, history: &History, evidence: &Evidence) -> Route {
    let figures = || [Effort::High, Effort::Xhigh].map(|effort| evidence.efforts.figures(effort, effort.arm())).join("; ");
    let (effort, arm, why) = if let Some(effort) = evidence.settled_planned_effort() {
        (effort, false, format!("at {} effort by the ledger's cost per kept box ({})", effort.name(), figures()))
    } else {
        let (effort, drawn) = drawn_effort(&route.category, item, history, evidence);
        (effort, true, format!("on effort arm {} {drawn} ({}; {ARM_RUNS} each to decide)", effort.arm(), figures()))
    };
    Route { tier: Tier::Opus, effort: Some(effort), effort_arm: arm, why: format!("{}, {why}", route.why), ..route }
}

fn drawn_effort(category: &str, item: &str, history: &History, evidence: &Evidence) -> (Effort, String) {
    let shares = evidence.shares(category, "effort");
    let (arm, drawn) = shares.draw(&history.seed("effort", item));
    (Effort::parse_arm(arm).unwrap_or(Effort::High), drawn)
}

/// The context arm this item's next launch draws from the category's context shares, and the words
/// naming the share it drew from.
pub fn context_arm(item: &str, category: &str, history: &History, evidence: &Evidence) -> (String, String) {
    let shares = evidence.shares(category, "context");
    let (arm, drawn) = shares.draw(&history.seed("context", item));
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
