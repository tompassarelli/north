//! The stopped sessions a capacity stop or a pending wait left holding unlanded lanes: each is
//! revived once its host's saturation refusal clears and placement and pace admit its account, as
//! a launch would be, and forgotten once its lanes' work is on origin/main. Until then it holds its
//! items, so no fresh launch starts the same work beside it.
use super::issues::IssueRef;
use super::plan::{self, Capacity};
use super::slots::Pools;
use crate::fleet::stranded::{Stop, Stranded};
use std::collections::BTreeMap;

/// A stranded session and the host whose registry holds it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Strand {
    pub session: Stranded,
    pub registry: String,
}

impl Strand {
    /// The machine its conversation and lanes live on.
    pub(crate) fn host(&self) -> &str {
        self.session.machine.as_deref().unwrap_or(&self.registry)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Revive {
    pub name: String,
    pub registry: String,
    pub host: String,
    pub account: String,
    pub stop: Stop,
    pub lanes: Vec<String>,
    /// The report the revive answers, which no later run revives again.
    pub ended_ms: u64,
    pub local: Option<String>,
    pub pin: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    Revive(Revive),
    /// Its lanes' work is on origin/main, so the session is forgotten.
    Forget { name: String, registry: String },
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Revival {
    pub steps: Vec<Step>,
    /// The items each unlanded strand holds, revived or not.
    pub holds: BTreeMap<IssueRef, String>,
    /// The strands left stopped this run, each with why.
    pub waiting: Vec<(String, String)>,
}

/// What a revive pass reads beside the slots and accounts: the strands, each host's saturation
/// refusal, and the report each session was last revived for.
pub(crate) struct Inputs<'a> {
    pub strands: &'a [Strand],
    pub saturated: &'a BTreeMap<String, String>,
    pub revived: &'a BTreeMap<String, u64>,
}

pub(crate) fn plan(inputs: &Inputs, capacity: &Capacity, pools: &mut Pools) -> Revival {
    let mut revival = Revival::default();
    for strand in inputs.strands {
        let session = &strand.session;
        if session.landed() {
            revival.steps.push(Step::Forget { name: session.name.clone(), registry: strand.registry.clone() });
            continue;
        }
        let items: Vec<IssueRef> = session.tasks.iter().filter_map(|task| IssueRef::parse(task)).collect();
        let held = |why: &str| format!("fleet session {} on {} stranded by {}: {why}", session.name, strand.registry, session.stop.label());
        match admit(strand, inputs, capacity, pools) {
            Ok(revive) => {
                revival.holds.extend(items.into_iter().map(|item| (item, held("revived by north dispatch"))));
                revival.steps.push(Step::Revive(revive));
            }
            Err(why) => {
                revival.holds.extend(items.into_iter().map(|item| (item, held(&why))));
                revival.waiting.push((session.name.clone(), why));
            }
        }
    }
    revival
}

fn admit(strand: &Strand, inputs: &Inputs, capacity: &Capacity, pools: &mut Pools) -> Result<Revive, String> {
    let session = &strand.session;
    if inputs.revived.get(&session.name).is_some_and(|revived| *revived >= session.ended_ms) {
        return Err("revived once for this report; it has not reported since".into());
    }
    let host = strand.host();
    if let Some(why) = inputs.saturated.get(host) {
        return Err(why.clone());
    }
    let account = format!("{}:{}", session.provider, session.account);
    let pin = plan::revive(capacity, pools, host, &account)?;
    let local = session.machine.is_none().then(|| format!("north dispatch revives {} where its conversation lives", session.name));
    let lanes = session.lanes.iter().filter(|lane| lane.unlanded()).map(|lane| lane.label()).collect();
    Ok(Revive { name: session.name.clone(), registry: strand.registry.clone(), host: host.to_owned(), account, stop: session.stop, lanes, ended_ms: session.ended_ms, local, pin })
}

/// The turn a revived session gets: why it runs again and what to finish.
pub(crate) fn prompt(revive: &Revive) -> String {
    format!(
        "north dispatch revived you: {} takes new work again, and your lane {} still holds work origin/main lacks. Your last report ended on {}. Continue from that report and land the lane through safe-push.",
        revive.host,
        revive.lanes.join(", "),
        revive.stop.label()
    )
}
