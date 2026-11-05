use super::free::{Gate, Shortfall};
use super::place::{Decision, Note, Refusal};
use super::rank::{Block, Tier, Warning};
use super::state::State;
use super::world::{Label, Unix};

pub fn explain(decision: &Decision, time: &dyn Fn(Unix) -> String) -> String {
    match decision {
        Decision::Place { account, tier, gate, state, notes } => {
            let mut line = format!("place {}: tier {}, {}{}", account.0, tier_number(*tier), describe_state(*state, time), crossed(*gate));
            for note in notes {
                line.push_str("; ");
                line.push_str(&describe_note(note, time));
            }
            line
        }
        Decision::Refuse(refusal) => format!("refuse: {}", describe_refusal(refusal, time)),
    }
}

fn tier_number(tier: Tier) -> u8 {
    match tier {
        Tier::One => 1,
        Tier::Two => 2,
        Tier::Three => 3,
    }
}

fn crossed(gate: Gate) -> &'static str {
    match gate {
        Gate::Pass { crossed: true } => ", reserve crossed by Tom's answer",
        Gate::Pass { crossed: false } | Gate::Reserve(_) => "",
    }
}

fn describe_state(state: State, time: &dyn Fn(Unix) -> String) -> String {
    match state {
        State::SignedOut => "signed out".to_owned(),
        State::WeekOut { resets_at } => format!("week out{}", resets(resets_at, time)),
        State::ScopeOut { family, resets_at } => {
            format!("{family:?} week out{}", resets(resets_at, time))
        }
        State::Paused { resumes_at } => format!("5h out{}", resets(resumes_at, time)),
        State::Unknown { read_at, .. } => match read_at {
            Some(read_at) => format!("not read since {}", time(read_at)),
            None => "not read".to_owned(),
        },
        State::Open { five_hour, week, not_started, .. } => {
            let five_hour = if not_started { "5h not started".to_owned() } else { format!("5h {:.0}%", five_hour.0) };
            format!("week {:.0}%, {five_hour}", week.0)
        }
    }
}

fn resets(at: Option<Unix>, time: &dyn Fn(Unix) -> String) -> String {
    at.map_or_else(String::new, |at| format!(" (resets {})", time(at)))
}

fn describe_note(note: &Note, time: &dyn Fn(Unix) -> String) -> String {
    match note {
        Note::Warning(Warning::MayPause { finish_by, resumes_at }) => {
            format!("may pause before it finishes (by {}){}", time(*finish_by), resumes_at.map_or_else(String::new, |at| format!(", back {}", time(at))))
        }
        Note::Warning(Warning::Unread { read_at }) => match read_at {
            Some(read_at) => format!("usage not read since {}", time(*read_at)),
            None => "usage never read".to_owned(),
        },
        Note::NotFirst { first } => format!("{} ranks first", first.0),
    }
}

fn describe_refusal(refusal: &Refusal, time: &dyn Fn(Unix) -> String) -> String {
    match refusal {
        Refusal::Tripped { workstream, spent, p90 } => format!("{} tripped its budget: spent ${:.2} against p90 ${:.2}", workstream.0, spent.0, p90.0),
        Refusal::UnknownAccount { pin } => format!("no account {}", pin.0),
        Refusal::Pinned { account, state, block, first } => {
            let why = block.map_or_else(|| "not signed in on the host".to_owned(), |block| describe_block(block, *state, time));
            let instead = first.as_ref().map_or_else(String::new, |first| format!("; {} could take it", first.0));
            format!("{} is pinned and {why}{instead}", account.0)
        }
        Refusal::NotOnHost { host, first } => format!("no account is signed in on {}; {} could take it elsewhere", host.0, first.0),
        Refusal::AllPaused { family, earliest_resume } => {
            format!("every account is out for {family:?}{}", earliest_resume.map_or_else(String::new, |at| format!(", earliest resume {}", time(at))))
        }
        Refusal::Reserve(shortfall) => reserve(shortfall),
        Refusal::ExtraUsage { accounts } => {
            format!("{} would pause into paid extra usage", join(accounts))
        }
        Refusal::NoneReadable { states } => {
            let each: Vec<String> = states.iter().map(|(label, state)| format!("{} {}", label.0, describe_state(*state, time))).collect();
            format!("no account is readable: {}", each.join(", "))
        }
    }
}

fn describe_block(block: Block, state: State, time: &dyn Fn(Unix) -> String) -> String {
    match block {
        Block::Closed(_) => describe_state(state, time),
        Block::Unread { .. } => format!("{} while a reserve applies", describe_state(state, time)),
        Block::ExtraUsage => "would pause into paid extra usage".to_owned(),
        Block::Reserve(shortfall) => reserve(&shortfall),
    }
}

fn reserve(shortfall: &Shortfall) -> String {
    format!(
        "the {:.0}% reserve would be crossed: {:.1}% free, {:.1}% in flight, {:.1}% for this box",
        shortfall.reserve, shortfall.free, shortfall.in_flight, shortfall.request
    )
}

fn join(labels: &[Label]) -> String {
    labels.iter().map(|label| label.0.as_str()).collect::<Vec<_>>().join(", ")
}
