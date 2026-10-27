//! Which worker tier a dispatched box starts on: Sonnet for art, Haiku for mechanical work with its
//! retry on Sonnet, the Sonnet and Opus arms in turn on the armed categories until the ledger holds
//! 15 runs of each, then the arm with the lower cost per closed box, and Opus for the rest.
use super::facets;
use super::issues::Issue;
use super::record::Record;
use crate::ledger::closed::ClosedReport;
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

/// Runs each arm needs in a category before dispatch routes it by the ledger's pick.
pub(crate) const ARM_RUNS: usize = 15;
/// Window of the ledger's closed boxes whose cost per closed box decides an armed category.
pub(crate) const ARM_WINDOW: &str = "7d";
/// Categories whose Sonnet and Opus arms dispatch compares.
pub(crate) const ARMED: [&str; 5] = ["feature", "tooling", "native-check", "balance-tuning", "bug-known-cause"];
const ART: [&str; 3] = ["anim", "animation", "art"];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Tier {
    Haiku,
    Sonnet,
    Opus,
}

impl Tier {
    /// The `[tiers]` key in South's orchestration, which is also the ledger's tier label.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Haiku => "claude:haiku",
            Self::Sonnet => "claude:sonnet-high",
            Self::Opus => "claude:opus-medium",
        }
    }

    /// The name the workers skill staffs it by.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Haiku => "worker-haiku",
            Self::Sonnet => "sonnet-high",
            Self::Opus => "worker",
        }
    }

    /// The word a ledger tier label carries for it.
    pub(crate) fn family(self) -> &'static str {
        match self {
            Self::Haiku => "haiku",
            Self::Sonnet => "sonnet",
            Self::Opus => "opus",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Haiku, Self::Sonnet, Self::Opus].into_iter().find(|tier| tier.key() == text || tier.name() == text)
    }

    fn other_arm(self) -> Self {
        if self == Self::Sonnet { Self::Opus } else { Self::Sonnet }
    }

    pub(crate) fn model_effort(self, cfg: &DocumentMut) -> Result<(String, String), String> {
        let table = cfg.get("tiers").and_then(|tiers| tiers.get(self.key()));
        let field = |name: &str| table.and_then(|tier| tier.get(name)).and_then(toml_edit::Item::as_str).map(str::to_owned);
        field("model").zip(field("effort")).ok_or_else(|| format!("orchestration has no model and effort for [tiers.\"{}\"]", self.key()))
    }

    pub(crate) fn invocation(self, cfg: &DocumentMut) -> Result<Vec<String>, String> {
        self.model_effort(cfg).map(|(model, effort)| vec!["--model".into(), model, "--effort".into(), effort])
    }

    pub(crate) fn serves(self, model: &str) -> bool {
        let family = match self {
            Self::Haiku => "haiku",
            Self::Sonnet => "sonnet",
            Self::Opus => "opus",
        };
        model.to_lowercase().contains(family)
    }
}

/// What the ledger holds for one category: each tier's success rate and runs there from
/// `--recommend`, and each tier's cost per closed box from `north ledger summary`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Evidence {
    pub tiers: BTreeMap<String, (f64, usize)>,
    pub usd_per_closed: Option<BTreeMap<Tier, f64>>,
}

impl Evidence {
    /// Reads `north worker-ledger --recommend 'Category: NAME'`; a coarser cell than the category counts
    /// as no evidence.
    pub(crate) fn parse(printed: &str, category: &str) -> Self {
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
        Self { tiers, usd_per_closed: None }
    }

    /// Adds each tier's cost per closed box in this category from the ledger's closed-box report,
    /// the figures `north ledger summary` prints; a tier with no closed box has no cost.
    pub(crate) fn priced(self, report: &ClosedReport, category: &str) -> Self {
        let mut sums: BTreeMap<Tier, (f64, usize)> = BTreeMap::new();
        for cell in report.cells.iter().filter(|cell| cell.category == category) {
            if let Some(tier) = [Tier::Haiku, Tier::Sonnet, Tier::Opus].into_iter().find(|tier| tier.serves(&cell.tier)) {
                let sum = sums.entry(tier).or_default();
                *sum = (sum.0 + cell.usd_per_closed() * cell.closed as f64, sum.1 + cell.closed);
            }
        }
        Self { usd_per_closed: Some(sums.into_iter().map(|(tier, (usd, closed))| (tier, usd / closed as f64)).collect()), ..self }
    }

    fn runs(&self, tier: Tier) -> usize {
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
    fn decided(&self) -> Option<Tier> {
        Some(if self.cost(Tier::Opus)? < self.cost(Tier::Sonnet)? { Tier::Opus } else { Tier::Sonnet })
    }

    /// The arm the category routes to once both arms hold `ARM_RUNS` runs and a cost; None while
    /// the arms still alternate.
    pub(crate) fn settled(&self) -> Option<Tier> {
        self.decided().filter(|_| self.runs(Tier::Sonnet) >= ARM_RUNS && self.runs(Tier::Opus) >= ARM_RUNS)
    }

    /// Both arms' runs behind the decision.
    pub(crate) fn arm_runs(&self) -> usize {
        self.runs(Tier::Sonnet) + self.runs(Tier::Opus)
    }

    pub(crate) fn figures(&self, tier: Tier) -> String {
        let cost = self.cost(tier).map_or_else(|| "no closed box".to_owned(), |usd| format!("${usd:.2} per closed box"));
        format!("{} {:.0}% of {}, {cost}", tier.name(), self.rate(tier), self.runs(tier))
    }
}

/// The tier one box launches on, the arm it records and the earlier launch it follows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Route {
    pub tier: Tier,
    pub category: String,
    pub arm: bool,
    pub follows: Option<String>,
    pub why: String,
}

/// Dispatch's earlier launches: each item's tiers and sessions, and each category's last arm.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct History {
    tried: BTreeMap<String, Vec<(Tier, String)>>,
    last_arm: BTreeMap<String, Tier>,
}

impl History {
    pub(crate) fn new(records: &[Record]) -> Self {
        let mut history = Self::default();
        let mut started: Vec<&Record> = records.iter().filter(|record| record.launched).collect();
        started.sort_by(|a, b| a.at.total_cmp(&b.at));
        for record in started {
            let Some(tier) = record.tier.as_deref().and_then(Tier::parse) else { continue };
            let route = Route { tier, category: record.category.clone().unwrap_or_default(), arm: record.arm, follows: None, why: String::new() };
            history.launched(&record.item, &record.name, &route);
        }
        history
    }

    pub(crate) fn launched(&mut self, item: &str, name: &str, route: &Route) {
        self.tried.entry(item.to_owned()).or_default().push((route.tier, name.to_owned()));
        self.chose(route);
    }

    /// Takes a route placed in this run, so the next box in its category gets the other arm.
    pub(crate) fn chose(&mut self, route: &Route) {
        if route.arm {
            self.last_arm.insert(route.category.clone(), route.tier);
        }
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

pub(crate) fn route(issue: &Issue, history: &History, ledger: &BTreeMap<String, Evidence>) -> Route {
    route_for(&issue.reference().to_string(), &|key| facets::facet(issue, key), history, ledger)
}

pub(crate) fn route_for(item: &str, facet: &dyn Fn(&str) -> String, history: &History, ledger: &BTreeMap<String, Evidence>) -> Route {
    let category = facet("Category").to_lowercase();
    let on = |tier: Tier, why: String| Route { tier, category: category.clone(), arm: false, follows: None, why };
    if art(&category, facet) {
        return on(Tier::Sonnet, "art goes to sonnet-high".into());
    }
    if category == "mechanical" {
        return match history.haiku_try(item) {
            Some(name) => Route { follows: Some(format!("{name} (worker-haiku)")), ..on(Tier::Sonnet, format!("retries the Haiku miss {name} on sonnet-high")) },
            None => on(Tier::Haiku, "mechanical work goes to worker-haiku".into()),
        };
    }
    if !ARMED.contains(&category.as_str()) {
        return on(Tier::Opus, format!("{category} stays on worker"));
    }
    let evidence = ledger.get(&category).cloned().unwrap_or_default();
    let (sonnet, opus) = (evidence.runs(Tier::Sonnet), evidence.runs(Tier::Opus));
    if let Some(tier) = evidence.settled() {
        return on(tier, format!("{category} goes to {} by the ledger's cost per closed box ({}; {})", tier.name(), evidence.figures(Tier::Sonnet), evidence.figures(Tier::Opus)));
    }
    let tier = history.last_arm.get(&category).map_or(Tier::Sonnet, |last| last.other_arm());
    Route { arm: true, ..on(tier, format!("{category} arm {} (sonnet-high n {sonnet}, worker n {opus}, {ARM_RUNS} each to decide)", tier.name())) }
}
