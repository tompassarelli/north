use super::free::{Gate, Shortfall};
use super::rank::{Block, Entry, Ranked, Tier, Verdict, Warning, rank};
use super::state::State;
use super::world::{EstimateSource, Host, Label, ModelFamily, Request, Trip, Unix, Usd, Workstream, World};

#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    Warning(Warning),
    NotFirst { first: Label },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    Tripped { workstream: Workstream, spent: Usd, p90: Usd },
    UnknownAccount { pin: Label },
    Pinned { account: Label, state: State, block: Option<Block>, first: Option<Label> },
    NotOnHost { host: Host, first: Label },
    AllPaused { family: ModelFamily, earliest_resume: Option<Unix> },
    Reserve(Shortfall),
    ExtraUsage { accounts: Vec<Label> },
    NoneReadable { states: Vec<(Label, State)> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Place { account: Label, tier: Tier, gate: Gate, state: State, notes: Vec<Note> },
    Refuse(Refusal),
}

pub fn place(world: &World, request: &Request) -> Decision {
    if let Some(trip) = tripped(world, request) {
        return Decision::Refuse(Refusal::Tripped { workstream: trip.workstream.clone(), spent: trip.spent, p90: trip.p90 });
    }
    let ranked = rank(world, request);
    let on_host = |entry: &&Entry| signed_in_on(world, &entry.account, request.host.as_ref());
    let first_here = ranked.placeable().find(on_host);
    let first_anywhere = ranked.placeable().next();
    if let Some(pin) = &request.pin {
        let Some(entry) = ranked.entry(pin) else {
            return Decision::Refuse(Refusal::UnknownAccount { pin: pin.clone() });
        };
        return match (entry.verdict, on_host(&entry)) {
            (Verdict::Placeable { .. }, true) => placed(entry, first_anywhere),
            (verdict, _) => Decision::Refuse(Refusal::Pinned {
                account: pin.clone(),
                state: entry.state,
                block: blocked(verdict),
                first: first_here.map(|entry| entry.account.clone()),
            }),
        };
    }
    match (first_here, first_anywhere) {
        (Some(entry), _) => placed(entry, None),
        (None, Some(first)) => Decision::Refuse(Refusal::NotOnHost { host: request.host.clone().unwrap_or(Host(String::new())), first: first.account.clone() }),
        (None, None) => Decision::Refuse(refusal(request, &ranked)),
    }
}

fn tripped<'a>(world: &'a World, request: &Request) -> Option<&'a Trip> {
    let workstream = request.workstream.as_ref()?;
    world.tripped.iter().find(|trip| &trip.workstream == workstream && !cleared_by_brief(request, trip))
}

fn cleared_by_brief(request: &Request, trip: &Trip) -> bool {
    matches!(request.expected.source, EstimateSource::Budget(_)) && request.expected.p90 > trip.p90
}

fn signed_in_on(world: &World, account: &Label, host: Option<&Host>) -> bool {
    let Some(host) = host else { return true };
    world.accounts.iter().any(|candidate| &candidate.label == account && candidate.signed_in.contains(host))
}

fn blocked(verdict: Verdict) -> Option<Block> {
    match verdict {
        Verdict::Blocked(block) => Some(block),
        Verdict::Placeable { .. } => None,
    }
}

fn placed(entry: &Entry, first: Option<&Entry>) -> Decision {
    let Verdict::Placeable { tier, warning, gate } = entry.verdict else { unreachable!("placed is called with a placeable entry") };
    let mut notes: Vec<Note> = warning.map(Note::Warning).into_iter().collect();
    if let Some(first) = first.filter(|first| first.account != entry.account) {
        notes.push(Note::NotFirst { first: first.account.clone() });
    }
    Decision::Place { account: entry.account.clone(), tier, gate, state: entry.state, notes }
}

fn refusal(request: &Request, ranked: &Ranked) -> Refusal {
    let states = || ranked.entries.iter().map(|entry| entry.state);
    let out = |state: State| matches!(state, State::Paused { .. } | State::WeekOut { .. } | State::ScopeOut { .. });
    if !ranked.entries.is_empty() && states().all(out) {
        return Refusal::AllPaused { family: request.family, earliest_resume: states().filter_map(resume_of).min() };
    }
    let shortfall = ranked.entries.iter().find_map(|entry| match entry.verdict {
        Verdict::Blocked(Block::Reserve(shortfall)) => Some(shortfall),
        _ => None,
    });
    if let Some(shortfall) = shortfall {
        return Refusal::Reserve(shortfall);
    }
    let extra: Vec<Label> =
        ranked.entries.iter().filter(|entry| matches!(entry.verdict, Verdict::Blocked(Block::ExtraUsage))).map(|entry| entry.account.clone()).collect();
    if !extra.is_empty() {
        return Refusal::ExtraUsage { accounts: extra };
    }
    Refusal::NoneReadable { states: ranked.entries.iter().map(|entry| (entry.account.clone(), entry.state)).collect() }
}

fn resume_of(state: State) -> Option<Unix> {
    match state {
        State::Paused { resumes_at } => resumes_at,
        State::WeekOut { resets_at } | State::ScopeOut { resets_at, .. } => resets_at,
        State::SignedOut | State::Unknown { .. } | State::Open { .. } => None,
    }
}
