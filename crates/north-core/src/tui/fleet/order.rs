use std::collections::BTreeSet;

use crate::fleet::model::Status;

use super::desk::SessionSummary;

pub fn arrange(mut sessions: Vec<SessionSummary>) -> Vec<SessionSummary> {
    sessions.sort_by(|left, right| left.record.created_ms.cmp(&right.record.created_ms).then_with(|| left.record.name.cmp(&right.record.name)));
    let (resting, active): (Vec<_>, Vec<_>) = sessions.into_iter().partition(|session| matches!(session.status, Status::Exited | Status::Parked));
    let mut arranged = tree(active);
    arranged.extend(tree(resting));
    arranged
}

fn tree(sessions: Vec<SessionSummary>) -> Vec<SessionSummary> {
    let names: BTreeSet<String> = sessions.iter().map(|session| session.record.name.clone()).collect();
    let mut arranged = Vec::with_capacity(sessions.len());
    for root in sessions.iter().filter(|session| session.record.parent.as_ref().is_none_or(|parent| !names.contains(parent) || parent == &session.record.name)) {
        descend(root, 0, &sessions, &mut arranged);
    }
    arranged
}

fn descend(session: &SessionSummary, depth: usize, all: &[SessionSummary], arranged: &mut Vec<SessionSummary>) {
    if arranged.iter().any(|placed| placed.record.name == session.record.name) {
        return;
    }
    arranged.push(SessionSummary { depth, ..session.clone() });
    for child in all.iter().filter(|child| child.record.parent.as_deref() == Some(session.record.name.as_str())) {
        descend(child, depth + 1, all, arranged);
    }
}
