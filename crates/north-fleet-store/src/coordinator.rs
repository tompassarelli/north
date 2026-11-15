use crate::runtime::COORDINATOR;
use crate::sender::Sender;

pub fn top_lead(session: &str, parent_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut top = None;
    let mut current = session.to_owned();
    while let Some(parent) = parent_of(&current).filter(|parent| parent != COORDINATOR) {
        current = parent.clone();
        top = Some(parent);
    }
    top
}

pub fn refusal(from: &Sender, to: &str, urgent: bool, parent_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    if urgent || *from != Sender::Session(COORDINATOR.to_owned()) {
        return None;
    }
    let lead = top_lead(to, parent_of)?;
    Some(format!("{to} works under the top lead {lead}; the coordinator speaks to top leads only, so send to {lead}, or send with --now to reach {to} directly"))
}
