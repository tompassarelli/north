use super::brief::HAIKU_WAIT;
use super::facets;
use super::record::{self, Record};
use super::survey;
use super::tier::{self, Evidence, History, Route, Tier};
use crate::ops::pacing::placement;
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

pub(crate) struct Ask<'a> {
    pub item: &'a str,
    pub brief: &'a str,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub reason: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Start {
    pub item: String,
    pub route: Route,
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub overridden: Option<String>,
    pub routed: bool,
}

pub(crate) fn start(ask: &Ask, cfg: &DocumentMut, history: &History, ledger: &BTreeMap<String, Evidence>) -> Result<Start, String> {
    let declared = |key: &str| facets::declared_in(ask.brief, key).map(str::to_owned);
    let facet = |key: &str| declared(key).unwrap_or_else(|| facets::default(key).to_owned());
    let named = declared("Tier").map(|name| named_tier(&name)).transpose()?;
    let route = match named {
        Some((tier, _)) => Route { tier, category: facet("Category").to_lowercase(), arm: false, follows: None, why: format!("the brief names {}", tier.name()) },
        None => tier::route_for(ask.item, &facet, history, ledger),
    };
    let (picked, tier_effort) = route.tier.model_effort(cfg)?;
    let effort = named.and_then(|(_, effort)| effort).map_or(tier_effort, str::to_owned);
    let (model, effort, overridden) = match ask.model {
        None => (picked, Some(effort), None),
        Some(given) if route.tier.serves(given) => (given.to_owned(), Some(effort), None),
        Some(given) => {
            let Some(reason) = ask.reason else {
                return Err(format!(
                    "--model {given} differs from {} ({}), the tier the ledger's category routing picks for {}; pass --tier-reason TEXT to run it anyway, or drop --model",
                    route.tier.name(),
                    route.why,
                    ask.item
                ));
            };
            (given.to_owned(), None, Some(reason.to_owned()))
        }
    };
    let prompt = if named.is_some() || overridden.is_some() { ask.prompt.to_owned() } else { stamped(ask.prompt, &route, declared("Category").is_none()) };
    Ok(Start { item: ask.item.to_owned(), route, model, effort, prompt, overridden, routed: named.is_some() })
}

fn named_tier(name: &str) -> Result<(Tier, Option<&'static str>), String> {
    match name {
        "worker-sonnet" => Ok((Tier::Sonnet, None)),
        "worker-high" => Ok((Tier::Opus, Some("high"))),
        "worker-xhigh" => Ok((Tier::Opus, Some("xhigh"))),
        other => Tier::parse(other).map(|tier| (tier, None)).ok_or_else(|| format!("the brief's `Tier: {other}` names no tier; use worker-haiku, worker-sonnet, sonnet-high, worker, worker-high or worker-xhigh")),
    }
}

fn stamped(prompt: &str, route: &Route, defaulted: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.extend(defaulted.then(|| format!("Category: {}", route.category)));
    lines.push(format!("Tier: {}", route.tier.name()));
    lines.extend(route.arm.then(|| format!("Arm: {}", route.tier.name())));
    lines.extend(route.follows.as_ref().map(|follows| format!("Follows: {follows}")));
    let all: Vec<&str> = prompt.lines().collect();
    let head = all.iter().position(|line| line.trim().is_empty()).unwrap_or(all.len());
    let header = all[..head].iter().any(|line| line.trim_start().starts_with("Item:") || line.trim_start().starts_with("Category:"));
    let mut text = if header {
        let mut kept: Vec<String> = all[..head].iter().map(|line| (*line).to_owned()).collect();
        kept.extend(lines);
        kept.extend(all[head..].iter().map(|line| (*line).to_owned()));
        kept.join("\n")
    } else {
        format!("{}\n\n{prompt}", lines.join("\n"))
    };
    if route.tier == Tier::Haiku {
        text.push_str(HAIKU_WAIT);
    }
    text
}

impl Start {
    pub(crate) fn record(&self, host: &str, account: &str, name: &str) -> Result<(), String> {
        if self.routed {
            return Ok(());
        }
        let overridden = self.overridden.is_some();
        let entry = Record {
            at: placement::now(),
            item: self.item.clone(),
            host: host.to_owned(),
            account: account.to_owned(),
            name: name.to_owned(),
            cloud: false,
            launched: true,
            detail: self.overridden.as_ref().map_or_else(|| self.route.why.clone(), |reason| format!("tier override: {reason}")),
            tier: (!overridden).then(|| self.route.tier.name().to_owned()),
            category: Some(self.route.category.clone()),
            arm: self.route.arm && !overridden,
            handoff: None,
            hand: true,
        };
        record::append(&record::directory(), &entry)
    }
}

pub(crate) fn begin(ask: &Ask) -> Result<Start, String> {
    let cfg = placement::config()?;
    let history = History::new(&record::read(&record::directory()));
    let category = facets::declared_in(ask.brief, "Category").unwrap_or_else(|| facets::default("Category")).to_lowercase();
    let ledger = if tier::ARMED.contains(&category.as_str()) { survey::evidence(&[category.as_str()], &mut Vec::new()) } else { BTreeMap::new() };
    start(ask, &cfg, &history, &ledger)
}
