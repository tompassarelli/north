use super::brief::HAIKU_WAIT;
use super::grounds::{self, Grounds};
use super::issues::{IssueRef, OWNER};
use crate::platform::github;
use super::facets;
use super::record::{self, Record};
use super::survey;
use super::tier::{self, Evidence, History, Route, Tier};
use crate::pacing::placement;
use std::collections::BTreeMap;
use toml_edit::DocumentMut;

pub struct Ask<'a> {
    pub item: &'a str,
    pub brief: &'a str,
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub worker: bool,
    /// A launch placement reads the item's host needs for, which `begin` reads into `Start::needs`.
    pub places: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub item: String,
    pub route: Route,
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub overridden: Option<String>,
    pub routed: bool,
    /// The needs of the item's boxes and the hosts its earlier workers reported (`host_needs::for_launch`), when `Ask::places`.
    pub needs: Vec<String>,
}

pub fn start(ask: &Ask, body: &str, cfg: &DocumentMut, history: &History, ledger: &BTreeMap<String, Evidence>, grounds: &dyn Fn(&Route, &'static str) -> Grounds) -> Result<Start, String> {
    if ask.worker && !body.trim().is_empty() && let Some(why) = super::issues::no_box_refusal(ask.item, body) {
        return Err(why);
    }
    let declared = |key: &str| facets::declared_in(ask.brief, key).map(str::to_owned);
    let facet = |key: &str| {
        if key == "Repro" {
            return facets::repro(&format!("{}\n{body}", ask.brief)).unwrap_or_default();
        }
        let value = declared(key).unwrap_or_else(|| facets::default(key).to_owned());
        if key == "Category" { facets::settled(value.to_lowercase(), &format!("{}\n{body}", ask.brief)) } else { value }
    };
    let named = declared("Tier").map(|name| named_tier(&name)).transpose()?;
    let route = match named {
        Some((tier, _)) => Route { tier, category: facet("Category").to_lowercase(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: format!("the brief names {}", tier.name()) },
        None => tier::route_for(ask.item, &facet, history, ledger)?,
    };
    let (picked, tier_effort) = route.model_effort(cfg)?;
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
            (given.to_owned(), Tier::Sonnet.serves(given).then(|| "high".to_owned()), Some(reason.to_owned()))
        }
    };
    let stamp = (named.is_none() && overridden.is_none()).then(|| declared("Category").is_none_or(|category| !category.eq_ignore_ascii_case(&route.category)));
    let launched = match (&overridden, ask.model) {
        (Some(_), Some(given)) => [Tier::Haiku, Tier::Sonnet, Tier::Opus].into_iter().find(|tier| tier.serves(given)),
        _ => Some(route.tier),
    };
    let family = launched.map(Tier::family).or_else(|| overridden.as_ref().and(ask.model).and_then(tier::family_of));
    let priced = Route { tier: launched.unwrap_or(route.tier), ..route.clone() };
    let prompt = stamped(ask.prompt, &priced, stamp, family.is_some(), &grounds(&priced, family.unwrap_or_else(|| priced.tier.family())));
    let prompt = if ask.worker { with_facets(&prompt) } else { prompt };
    let (prompt, route) = context_armed(ask, prompt, route, overridden.is_some(), history, ledger);
    Ok(Start { item: ask.item.to_owned(), route, model, effort, prompt, overridden, routed: named.is_some(), needs: Vec::new() })
}

/// A launch whose brief names no arm draws the context arm dispatch draws for an unarmed local box, whoever
/// launches it and whatever model it names; a tier override stamps no effort arm, so it draws too.
fn context_armed(ask: &Ask, prompt: String, route: Route, overridden: bool, history: &History, ledger: &BTreeMap<String, Evidence>) -> (String, Route) {
    let declared = [ask.brief, prompt.as_str()].iter().any(|text| text.lines().any(|line| line.trim_start().starts_with("Arm:")));
    if declared || (route.stamp().is_some() && !overridden) {
        return (prompt, route);
    }
    let evidence = ledger.get(&route.category).cloned().unwrap_or_default();
    let (arm, drawn) = tier::context_arm(ask.item, &route.category, history, &evidence);
    let (armed, why) = super::pack::armed(&prompt, (&arm, &drawn), ask.item, &route.category, history, &evidence);
    (armed, Route { why: format!("{}, {why}", route.why), ..route })
}

/// The auto-compact window a launch that names no issue, the proxy's own included, draws from its category's context shares.
pub fn untasked_window(name: &str, category: &str, history: &History, ledger: &BTreeMap<String, Evidence>) -> Option<u64> {
    let evidence = ledger.get(category).cloned().unwrap_or_default();
    let (arm, drawn) = tier::context_arm(name, category, history, &evidence);
    eprintln!("tier: {name} context arm {arm} {drawn}");
    crate::fleet::compaction::window(&format!("Arm: {arm}"))
}

/// [`untasked_window`] over the ledger and launch history as `north fleet launch` reads them.
pub fn draw_untasked_window(name: &str, brief: &str) -> Option<u64> {
    let category = facets::declared_in(brief, "Category").unwrap_or_else(|| facets::default("Category")).to_lowercase();
    let history = History::new(&record::read(&record::directory()));
    let mut notes = Vec::new();
    let ledger = survey::evidence(&[category.as_str()], &mut notes);
    notes.iter().for_each(|note| eprintln!("north fleet: {note}"));
    untasked_window(name, &category, &history, &ledger)
}

fn named_tier(name: &str) -> Result<(Tier, Option<&'static str>), String> {
    match name {
        "worker-sonnet" => Ok((Tier::Sonnet, None)),
        "worker-high" => Ok((Tier::Opus, Some("high"))),
        "worker-xhigh" => Ok((Tier::Opus, Some("xhigh"))),
        other => Tier::parse(other).map(|tier| (tier, None)).ok_or_else(|| format!("the brief's `Tier: {other}` names no tier; use worker-haiku, worker-sonnet, sonnet-high, worker, worker-high or worker-xhigh")),
    }
}

/// The brief with the facets its header leaves out at the defaults dispatch assumes for an issue that names none,
/// so a hand launch's run records them.
fn with_facets(prompt: &str) -> String {
    let all: Vec<&str> = prompt.lines().collect();
    let head = all.iter().position(|line| line.trim().is_empty()).unwrap_or(all.len());
    let missing: Vec<String> = ["Spec", "Scope", "Surface", "Verify"].iter().filter(|key| !all[..head].iter().any(|line| facets::declared_in(line, key).is_some())).map(|key| format!("{key}: {}", facets::default(key))).collect();
    if missing.is_empty() {
        return prompt.to_owned();
    }
    if !all[..head].iter().any(|line| ["Item:", "Category:", "Tier:"].iter().any(|name| line.trim_start().starts_with(name))) {
        return format!("{}\n\n{prompt}", missing.join("\n"));
    }
    let mut kept: Vec<String> = all[..head].iter().map(|line| (*line).to_owned()).collect();
    kept.extend(missing);
    kept.extend(all[head..].iter().map(|line| (*line).to_owned()));
    kept.join("\n")
}

fn stamped(prompt: &str, route: &Route, defaulted: Option<bool>, calibrated: bool, grounds: &Grounds) -> String {
    let all: Vec<&str> = prompt.lines().collect();
    let head = all.iter().position(|line| line.trim().is_empty()).unwrap_or(all.len());
    let mut lines: Vec<String> = Vec::new();
    let measured = calibrated && grounds.measured();
    let declared_eta = all[..head].iter().find_map(|line| facets::declared_in(line, "ETA"));
    let restated = |line: &&str| (defaulted == Some(true) && facets::declared_in(line, "Category").is_some()) || (measured && facets::declared_in(line, "ETA").is_some());
    if let Some(defaulted) = defaulted {
        lines.extend(defaulted.then(|| format!("Category: {}", route.category)));
        lines.push(format!("Tier: {}", route.tier.name()));
        lines.extend(route.stamp().map(|arm| format!("Arm: {arm}")));
        lines.extend(route.follows.as_ref().map(|follows| format!("Follows: {follows}")));
    }
    if measured {
        let ledger = grounds.eta(None, route);
        lines.push(format!("ETA: {}", grounds.eta(declared_eta.filter(|declared| *declared != ledger), route)));
        lines.extend(grounds.budget(route));
    }
    lines.extend(grounds.trace());
    let header = all[..head].iter().any(|line| line.trim_start().starts_with("Item:") || line.trim_start().starts_with("Category:"));
    let mut text = if lines.is_empty() {
        prompt.to_owned()
    } else if header {
        let mut kept: Vec<String> = all[..head].iter().filter(|line| !restated(line)).map(|line| (*line).to_owned()).collect();
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
    pub fn entry(&self, host: &str, account: &str, name: &str) -> Option<Record> {
        if self.routed {
            return None;
        }
        let overridden = self.overridden.is_some();
        Some(Record {
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
            box_text: None,
            hand: true,
            effort_arm: self.route.stamp().filter(|_| !overridden).map(str::to_owned),
            signal: self.route.signal.filter(|_| !overridden).map(|signal| signal.key().to_owned()),
            lift: None,
            effort: self.route.effort.filter(|_| !overridden).map(|effort| effort.name().to_owned()),
            cell: super::cells::named(&self.model, self.effort.as_deref().unwrap_or_default()),
            cell_role: self.route.cell_role.filter(|_| !overridden).map(str::to_owned),
            route: None,
        })
    }

    pub fn record(&self, host: &str, account: &str, name: &str, route: Option<&str>) -> Result<(), String> {
        self.entry(host, account, name).map_or(Ok(()), |entry| record::append(&record::directory(), &Record { route: route.map(str::to_owned), ..entry }))
    }
}

pub fn begin(ask: &Ask, issue: Result<(String, f64), String>) -> Result<Start, String> {
    let cfg = placement::config()?;
    let (body, created) = if IssueRef::parse(ask.item).is_none() {
        (String::new(), 0.0)
    } else {
        issue.unwrap_or_else(|why| {
            eprintln!("north fleet: {} unread, so its brief carries no Not required lines or prior reports: {why}", ask.item);
            (String::new(), f64::INFINITY)
        })
    };
    let category = facets::settled(facets::declared_in(ask.brief, "Category").unwrap_or_else(|| facets::default("Category")).to_lowercase(), &format!("{}\n{body}", ask.brief));
    let (history, (ledger, mut notes), (grounds, grounds_notes), (needs, needs_notes)) = std::thread::scope(|scope| {
        let history = scope.spawn(|| History::new(&record::read(&record::directory())));
        let ledger = scope.spawn(|| noted(|notes| survey::evidence(&[category.as_str()], notes)));
        let grounds = scope.spawn(|| noted(grounds::Ledger::recorded));
        let needs = noted(|notes| IssueRef::parse(ask.item).filter(|_| ask.places).map(|item| super::host_needs::for_launch(&item, &body, facets::declared_in(ask.brief, "Box"), placement::now(), notes)).unwrap_or_default());
        (joined(history), joined(ledger), joined(grounds), needs)
    });
    notes.extend(grounds_notes.into_iter().chain(needs_notes));
    notes.iter().for_each(|note| eprintln!("north fleet: {note}"));
    start(ask, &body, &cfg, &history, &ledger, &|route, family| grounds.grounds(ask.item, created, &body, route, family)).map(|start| Start { needs, ..start })
}

/// The tier route() picks for a worker brief, for a spawn guard that compares it with the tier asked for.
pub fn picked_tier(brief: &str) -> Result<Tier, String> {
    let item = facets::declared_in(brief, "Item").unwrap_or("agent spawn");
    begin(&Ask { item, brief, prompt: brief, model: None, reason: None, worker: false, places: false }, issue_text(item)).map(|start| start.route.tier)
}

/// The issue's body and when it was opened, as unix seconds.
pub fn issue_text(item: &str) -> Result<(String, f64), String> {
    let item = IssueRef::parse(item).ok_or_else(|| format!("{item} names no issue"))?;
    let json = github::json(&["issue", "view", &item.number.to_string(), "-R", &format!("{OWNER}/{}", item.repo), "--json", "body,createdAt"])?;
    let created = json["createdAt"].as_str().and_then(crate::foundation::clock::rfc3339_seconds).ok_or_else(|| format!("{item} carries no createdAt"))?;
    Ok((json["body"].as_str().unwrap_or_default().to_owned(), created))
}

pub fn tier_reason(brief: &str, flag: Option<&str>) -> Option<String> {
    flag.or_else(|| facets::declared_in(brief, "Tier-reason")).map(str::to_owned)
}

pub fn untasked(brief: &str, model: Option<&str>, reason: Option<&str>) -> Result<bool, String> {
    if let Some(model) = model {
        return match reason {
            Some(_) => Ok(false),
            None => Err(format!(
                "--model {model} names a tier, but the launch names no issue (--task or a first `Item:` line) whose category routing could check it; add a `Tier-reason:` line to the brief header, pass --tier-reason TEXT, or name the issue"
            )),
        };
    }
    let declared = |key: &str| facets::declared_in(brief, key);
    let worker = declared("Delegation").is_some_and(|delegation| delegation.contains("role=worker")) || ["Spec", "Scope", "Surface", "Verify"].iter().any(|key| declared(key).is_some());
    let worker = worker && declared("Item").is_none();
    match (declared("Category").or_else(|| declared("Tier")), worker) {
        (Some(_), _) => Ok(true),
        (None, true) => Err("this worker launch names no issue, no `Category:` or `Tier:` line and no --model, so no tier can be picked for it; add a `Category:` line to the brief header, or name the issue (`--task` or an `Item:` line)".to_owned()),
        (None, false) => Ok(false),
    }
}

fn noted<T>(read: impl FnOnce(&mut Vec<String>) -> T) -> (T, Vec<String>) {
    let mut notes = Vec::new();
    (read(&mut notes), notes)
}

fn joined<T>(thread: std::thread::ScopedJoinHandle<'_, T>) -> T {
    thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
