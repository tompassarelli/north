//! The session tree follows the issue tree (north#1137): a parent issue whose subtree holds more ready or running work
//! than one session's reports can be read for gets one lead, named `lead-REPO-N`; every box and leaf under a live lead
//! launches with that lead as its parent; and a lead dispatch launched stops once its parent closes or nothing under it
//! is ready and no child of it lives. Dispatch staffs and stops leads only while the orchestration config sets both bounds.
use super::fleets::Session;
use super::issues::{Issue, IssueRef, OWNER};
use super::record::BoxHolds;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use toml_edit::DocumentMut;

const PREFIX: &str = "lead-";
const REFERENCE: &str = "harness/skills/workers/references/lead.md";
/// The existing category a lead routes and is ledgered under, since a lead reviews and steers its children's work.
pub const CATEGORY: &str = "review";

/// The name of the lead dispatch staffs for `item`, which refuses a second lead for one parent.
pub fn name(item: &IssueRef) -> String {
    format!("{PREFIX}{}-{}", item.repo, item.number)
}

/// The parent a live session leads: its own by the name dispatch gives a lead, else the first item of a session the registry lists with role lead.
fn led_by(session: &Session) -> Option<IssueRef> {
    named(&session.name).or_else(|| session.items.first().filter(|_| session.lead).cloned())
}

fn named(session: &str) -> Option<IssueRef> {
    let (repo, number) = session.strip_prefix(PREFIX)?.rsplit_once('-')?;
    let number = number.parse().ok()?;
    (!repo.is_empty()).then(|| IssueRef::new(repo, number))
}

/// The orchestration config's `[dispatch] lead_reports`, the load over which a parent gets a lead, and `[dispatch]
/// leads`, the leads dispatch launched that live at once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub reports: usize,
    pub leads: usize,
}

impl Bounds {
    /// The bounds `cfg` sets, none while either key is unset.
    pub fn configured(cfg: &DocumentMut) -> Option<Self> {
        let number = |key: &str| cfg.get("dispatch")?.get(key)?.as_integer().and_then(|value| usize::try_from(value).ok());
        Some(Self { reports: number("lead_reports")?, leads: number("leads")? })
    }
}

/// What the survey read that a lead's load and stop rest on.
pub struct Inputs<'a> {
    pub issues: &'a [Issue],
    pub open: &'a BTreeSet<IssueRef>,
    pub sessions: &'a [(Session, String)],
    pub holds: &'a BTreeMap<IssueRef, String>,
    pub box_holds: &'a BoxHolds,
    pub at: f64,
}

/// A parent with no live lead whose load is over the bound.
#[derive(Clone, Debug, PartialEq)]
pub struct Over {
    pub item: IssueRef,
    pub load: usize,
    pub bound: usize,
}

impl fmt::Display for Over {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lead {}: load {} over {}", self.item, self.load, self.bound)
    }
}

/// A live lead this run stops, with the registry that lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub name: String,
    pub registry: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    /// The parents over the bound that no live session leads or holds, the largest load first.
    pub over: Vec<Over>,
    pub stops: Vec<Stop>,
    /// The dispatched leads that may start this run.
    pub room: usize,
    up: BTreeMap<IssueRef, IssueRef>,
    leading: BTreeMap<IssueRef, String>,
}

/// Each node's load, and the ready units anywhere under it.
#[derive(Default)]
struct Loads {
    load: BTreeMap<IssueRef, usize>,
    ready: BTreeMap<IssueRef, usize>,
}

impl Tree {
    /// Reads each open parent's load: the boxes and leaves of it and its descendants that `ready` admits this run or a
    /// live dispatched session holds, less those under a deeper parent with its own live lead. With no bounds the tree is
    /// empty, so dispatch staffs, stops and reparents nothing.
    pub fn read(inputs: &Inputs, ready: impl Fn(&Issue) -> bool, bounds: Option<Bounds>) -> Self {
        let Some(bounds) = bounds else { return Self::default() };
        let up: BTreeMap<IssueRef, IssueRef> = inputs.issues.iter().filter_map(|issue| issue.parent.as_ref().filter(|parent| parent.owner == OWNER).map(|parent| (issue.reference(), IssueRef::new(&parent.repo, parent.number)))).collect();
        let live: BTreeMap<IssueRef, (&Session, &str)> = inputs.sessions.iter().filter_map(|(session, registry)| led_by(session).map(|item| (item, (session, registry.as_str())))).collect();
        let mut leading: BTreeMap<IssueRef, String> = live.iter().map(|(item, (session, _))| (item.clone(), session.name.clone())).collect();
        let loads = loads(inputs, &up, &leading, ready);
        let mut over: Vec<Over> = inputs
            .issues
            .iter()
            .filter(|issue| issue.children.total > 0 && !leading.contains_key(&issue.reference()) && !inputs.holds.contains_key(&issue.reference()))
            .map(|issue| Over { item: issue.reference(), load: loads.load.get(&issue.reference()).copied().unwrap_or(0), bound: bounds.reports })
            .filter(|parent| parent.load > parent.bound)
            .collect();
        over.sort_by(|a, b| b.load.cmp(&a.load).then_with(|| a.item.cmp(&b.item)));
        over.dedup();
        let stops = stops(inputs, &live, &loads.ready);
        leading.retain(|_, name| !stops.iter().any(|stop| stop.name == *name));
        let dispatched = live.values().filter(|(session, _)| session.dispatched).count();
        let room = bounds.leads.saturating_sub(dispatched - stops.len());
        Self { over, stops, room, up, leading }
    }

    /// The live lead of `item`'s nearest leading ancestor, `item` itself included, which reads a launch's reports.
    pub fn reader(&self, item: &IssueRef) -> Option<String> {
        chain(&self.up, item.clone()).into_iter().find_map(|node| self.leading.get(&node).cloned())
    }

    /// The live lead above `item`, which reads the reports of `item`'s own lead.
    pub fn reader_above(&self, item: &IssueRef) -> Option<String> {
        self.up.get(item).and_then(|parent| self.reader(parent))
    }
}

fn loads(inputs: &Inputs, up: &BTreeMap<IssueRef, IssueRef>, leading: &BTreeMap<IssueRef, String>, ready: impl Fn(&Issue) -> bool) -> Loads {
    let mut loads = Loads::default();
    let mut seen = BTreeSet::new();
    for issue in inputs.issues.iter().filter(|issue| seen.insert(issue.reference())) {
        let admitted = ready(issue);
        let units = units(inputs, issue, admitted);
        let mut counting = true;
        for node in chain(up, issue.reference()) {
            if admitted {
                *loads.ready.entry(node.clone()).or_default() += units;
            }
            if counting {
                counting = !leading.contains_key(&node);
                *loads.load.entry(node).or_default() += units;
            }
        }
    }
    loads
}

/// The leads dispatch launched whose parent is closed, or under which nothing is ready and no child lives; a lead without
/// dispatch's launch mark stays whatever its name, and so does a lead of a repository the survey read no issue of, since
/// its parent's state is unread.
fn stops(inputs: &Inputs, live: &BTreeMap<IssueRef, (&Session, &str)>, ready: &BTreeMap<IssueRef, usize>) -> Vec<Stop> {
    let surveyed: BTreeSet<&str> = inputs.issues.iter().map(|issue| issue.repo.as_str()).collect();
    let mut stops = Vec::new();
    for (item, (lead, registry)) in live.iter().filter(|(item, (lead, _))| surveyed.contains(item.repo.as_str()) && lead.dispatched) {
        let quiet = ready.get(item).copied().unwrap_or(0) == 0 && !inputs.sessions.iter().any(|(session, _)| session.parent.as_deref() == Some(lead.name.as_str()));
        let why = match (inputs.open.contains(item), quiet) {
            (false, _) => format!("its parent {item} is closed"),
            (true, true) => format!("nothing under {item} is ready and no child of it lives"),
            (true, false) => continue,
        };
        stops.push(Stop { name: lead.name.clone(), registry: (*registry).to_owned(), why });
    }
    stops
}

/// `item` and its ancestors, nearest first, as far as the survey read them.
fn chain(up: &BTreeMap<IssueRef, IssueRef>, item: IssueRef) -> Vec<IssueRef> {
    let mut nodes = vec![item];
    while let Some(parent) = nodes.last().and_then(|node| up.get(node)).filter(|parent| !nodes.contains(parent)) {
        nodes.push(parent.clone());
    }
    nodes
}

/// The units of work `issue` adds to its ancestors' load: each takeable box when it is ready (a bare leaf is one),
/// else each box or the whole issue a live dispatched session holds.
fn units(inputs: &Inputs, issue: &Issue, ready: bool) -> usize {
    if ready {
        return issue.takeable(inputs.at).count().max(1);
    }
    let fleet = |holder: &String| holder.strip_prefix("fleet session ").is_some_and(|rest| !rest.starts_with(PREFIX));
    let item = issue.reference();
    inputs.box_holds.get(&item).map_or(0, |held| held.values().filter(|holder| fleet(holder)).count()) + usize::from(inputs.holds.get(&item).is_some_and(fleet))
}

/// The lead's brief: the header a worker brief for its parent carries, then what a lead does.
pub fn brief(worker_brief: &str, over: &Over, launched_by: &str) -> String {
    let head = worker_brief.split("\n\n").next().unwrap_or(worker_brief).lines().map(|line| match line.split_once(':').map(|(key, _)| key) {
        Some("Category") => format!("Category: {CATEGORY}"),
        Some("ETA") => "ETA: n/a".to_owned(),
        _ => line.to_owned(),
    });
    let head = head.collect::<Vec<_>>().join("\n");
    let item = &over.item;
    format!(
        "{head}\n\n{launched_by} staffed you as the lead of {item}, named {}: its open boxes and sub-issues hold {} ready or running units of work, over the {} whose reports one session reads. \
         Read {REFERENCE} in the north repository and lead as it says. Dispatch launches each box and leaf under {item} with you as its parent, so their reports reach you; \
         read them, steer and unblock them, and claim or work no box yourself. Dispatch stops you once {item} closes, or once nothing under it is ready and no child of yours lives. \
         End each turn with Done:, Not done: or Split:, and Stop: with its reason.",
        name(item),
        over.load,
        over.bound,
    )
}

/// What stopping `stops` prints, stopping each one unless `dry_run`.
pub fn stop_all(stops: &[Stop], dry_run: bool, stop: impl Fn(&Stop) -> Result<(), String>) -> Vec<String> {
    stops
        .iter()
        .map(|lead| {
            let line = format!("stop {} on {}: {}", lead.name, lead.registry, lead.why);
            if dry_run {
                return format!("would {line}");
            }
            match stop(lead) {
                Ok(()) => line,
                Err(why) => format!("{line} failed: {why}"),
            }
        })
        .collect()
}
