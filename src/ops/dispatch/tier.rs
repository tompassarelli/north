//! Which worker tier a dispatched box starts on: Sonnet for known-cause bugs and art, Haiku for
//! mechanical work with its retry on Sonnet, the Sonnet and Opus arms in turn on tooling and
//! features until the ledger holds 15 runs of each, then the ledger's pick, and Opus for the rest.
use super::facets;
use super::issues::Issue;
use super::record::Record;
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

/// Runs each arm needs in a category before dispatch routes it by the ledger's pick.
pub(crate) const ARM_RUNS: usize = 15;
/// Categories whose Sonnet and Opus arms dispatch compares.
pub(crate) const ARMED: [&str; 2] = ["tooling", "feature"];
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

    fn parse(text: &str) -> Option<Self> {
        [Self::Haiku, Self::Sonnet, Self::Opus].into_iter().find(|tier| tier.key() == text || tier.name() == text)
    }

    fn other_arm(self) -> Self {
        if self == Self::Sonnet { Self::Opus } else { Self::Sonnet }
    }

    /// `--model M --effort E` from the tier's `[tiers]` table in South's orchestration.
    pub(crate) fn invocation(self, cfg: &DocumentMut) -> Result<Vec<String>, String> {
        let table = cfg.get("tiers").and_then(|tiers| tiers.get(self.key()));
        let field = |name: &str| table.and_then(|tier| tier.get(name)).and_then(toml_edit::Item::as_str).map(str::to_owned);
        match (field("model"), field("effort")) {
            (Some(model), Some(effort)) => Ok(vec!["--model".into(), model, "--effort".into(), effort]),
            _ => Err(format!("orchestration has no model and effort for [tiers.\"{}\"]", self.key())),
        }
    }
}

/// What the ledger's `--recommend` printed for one category: each tier's success rate and runs
/// there, and its pick when one passes or has a measured cost.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Evidence {
    pub tiers: BTreeMap<String, (f64, usize)>,
    pub pick: Option<String>,
}

impl Evidence {
    /// Reads `worker-ledger --recommend 'Category: NAME'`; a coarser cell than the category counts
    /// as no evidence.
    pub(crate) fn parse(printed: &str, category: &str) -> Self {
        let Some((head, cell)) = printed.lines().find_map(|line| line.split_once("  cell [")) else { return Self::default() };
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
        Self { tiers, pick: head.strip_prefix("tier ").map(|pick| pick.trim().to_owned()) }
    }

    fn runs(&self, tier: Tier) -> usize {
        self.tiers.get(tier.key()).map_or(0, |(_, runs)| *runs)
    }

    fn rate(&self, tier: Tier) -> f64 {
        self.tiers.get(tier.key()).map_or(0.0, |(rate, _)| *rate)
    }

    /// The arm the ledger favors: its pick when that is an arm, else the arm with the higher
    /// success rate, Sonnet on a tie since it costs less.
    fn decided(&self) -> Tier {
        match self.pick.as_deref().and_then(Tier::parse) {
            Some(tier @ (Tier::Sonnet | Tier::Opus)) => tier,
            _ if self.rate(Tier::Opus) > self.rate(Tier::Sonnet) => Tier::Opus,
            _ => Tier::Sonnet,
        }
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

fn art(category: &str, issue: &Issue) -> bool {
    let judged = facets::facet(issue, "Spec").eq_ignore_ascii_case("judged");
    let pictured = facets::facet(issue, "Surface").eq_ignore_ascii_case("assets") || facets::facet(issue, "Verify").eq_ignore_ascii_case("visual");
    ART.contains(&category) || judged && pictured
}

pub(crate) fn route(issue: &Issue, history: &History, ledger: &BTreeMap<String, Evidence>) -> Route {
    let category = facets::facet(issue, "Category").to_lowercase();
    let item = issue.reference().to_string();
    let on = |tier: Tier, why: String| Route { tier, category: category.clone(), arm: false, follows: None, why };
    if art(&category, issue) {
        return on(Tier::Sonnet, "art goes to sonnet-high".into());
    }
    if category == "bug-known-cause" {
        return on(Tier::Sonnet, "known-cause bugs go to sonnet-high".into());
    }
    if category == "mechanical" {
        return match history.haiku_try(&item) {
            Some(name) => Route { follows: Some(format!("{name} (worker-haiku)")), ..on(Tier::Sonnet, format!("retries the Haiku miss {name} on sonnet-high")) },
            None => on(Tier::Haiku, "mechanical work goes to worker-haiku".into()),
        };
    }
    if !ARMED.contains(&category.as_str()) {
        return on(Tier::Opus, format!("{category} stays on worker"));
    }
    let evidence = ledger.get(&category).cloned().unwrap_or_default();
    let (sonnet, opus) = (evidence.runs(Tier::Sonnet), evidence.runs(Tier::Opus));
    if sonnet >= ARM_RUNS && opus >= ARM_RUNS {
        let tier = evidence.decided();
        let pick = evidence.pick.as_deref().unwrap_or("no tier");
        return on(tier, format!("{category} goes to {} by the ledger, which picks {pick} (sonnet-high n {sonnet}, worker n {opus})", tier.name()));
    }
    let tier = history.last_arm.get(&category).map_or(Tier::Sonnet, |last| last.other_arm());
    Route { arm: true, ..on(tier, format!("{category} arm {} (sonnet-high n {sonnet}, worker n {opus}, {ARM_RUNS} each to decide)", tier.name())) }
}
