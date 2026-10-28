use super::brief::HAIKU_WAIT;
use super::grounds::{self, Grounds};
use super::issues::{IssueRef, OWNER};
use crate::platform::github;
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

pub(crate) fn start(ask: &Ask, cfg: &DocumentMut, history: &History, ledger: &BTreeMap<String, Evidence>, grounds: &dyn Fn(&Route) -> Grounds) -> Result<Start, String> {
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
    let stamp = (named.is_none() && overridden.is_none()).then(|| declared("Category").is_none());
    let prompt = stamped(ask.prompt, &route, stamp, overridden.is_none(), &grounds(&route));
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

fn stamped(prompt: &str, route: &Route, defaulted: Option<bool>, calibrated: bool, grounds: &Grounds) -> String {
    let all: Vec<&str> = prompt.lines().collect();
    let head = all.iter().position(|line| line.trim().is_empty()).unwrap_or(all.len());
    let mut lines: Vec<String> = Vec::new();
    if let Some(defaulted) = defaulted {
        lines.extend(defaulted.then(|| format!("Category: {}", route.category)));
        lines.push(format!("Tier: {}", route.tier.name()));
        lines.extend(route.arm.then(|| format!("Arm: {}", route.tier.name())));
        lines.extend(route.follows.as_ref().map(|follows| format!("Follows: {follows}")));
    }
    if calibrated && grounds.measured() {
        let declared = all[..head].iter().any(|line| facets::declared_in(line, "ETA").is_some());
        lines.push(format!("{}: {}", if declared { "Ledger ETA" } else { "ETA" }, grounds.eta(None, route)));
        lines.extend(grounds.budget(route));
    }
    lines.extend(grounds.trace());
    let header = all[..head].iter().any(|line| line.trim_start().starts_with("Item:") || line.trim_start().starts_with("Category:"));
    let mut text = if lines.is_empty() {
        prompt.to_owned()
    } else if header {
        let mut kept: Vec<String> = all[..head].iter().map(|line| (*line).to_owned()).collect();
        kept.extend(lines);
        kept.extend(all[head..].iter().map(|line| (*line).to_owned()));
        kept.join("\n")
    } else {
        format!("{}\n\n{prompt}", lines.join("\n"))
    };
    if defaulted.is_some() && route.tier == Tier::Haiku {
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
    let mut notes = Vec::new();
    let grounds = grounds::Ledger::read(&mut notes);
    notes.iter().for_each(|note| eprintln!("north fleet: {note}"));
    let body = issue_body(ask.item).unwrap_or_else(|why| {
        eprintln!("north fleet: {} body unread, so its brief carries no Not required lines: {why}", ask.item);
        String::new()
    });
    start(ask, &cfg, &history, &ledger, &|route| grounds.grounds(ask.item, &body, route))
}

fn issue_body(item: &str) -> Result<String, String> {
    let item = IssueRef::parse(item).ok_or_else(|| format!("{item} names no issue"))?;
    let json = github::json(&["issue", "view", &item.number.to_string(), "-R", &format!("{OWNER}/{}", item.repo), "--json", "body"])?;
    Ok(json["body"].as_str().unwrap_or_default().to_owned())
}

pub(crate) fn tier_reason(brief: &str, flag: Option<&str>) -> Option<String> {
    flag.or_else(|| facets::declared_in(brief, "Tier-reason")).map(str::to_owned)
}

pub(crate) fn unrouted(model: Option<&str>, reason: Option<&str>) -> Result<(), String> {
    match (model, reason) {
        (Some(model), None) => Err(format!(
            "--model {model} names a tier, but the launch names no issue (--task or a first `Item:` line) whose category routing could check it; add a `Tier-reason:` line to the brief header, pass --tier-reason TEXT, or name the issue"
        )),
        _ => Ok(()),
    }
}
