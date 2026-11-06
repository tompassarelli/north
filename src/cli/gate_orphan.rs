use serde_json::Value;

use crate::dispatch::{IssueRef, OWNER, references};
use crate::fleet::gates::Gate;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::github_mirror::store::{Mirror, OPEN_ISSUES, key};

pub(in crate::cli) fn close_orphaned(fleet: &Fleet) {
    let closed = match fleet.close_orphaned_gates(issues_closed) {
        Ok(closed) => closed,
        Err(error) => return eprintln!("north gate: orphaned gates not closed: {}", error.user_message()),
    };
    for (id, outcome) in closed {
        match outcome {
            Ok(closure) => eprintln!("north gate {id}: {}", closure.answer),
            Err(error) => eprintln!("north gate {id}: {}", error.user_message()),
        }
    }
}

fn issues_closed(gate: &Gate) -> bool {
    let named: Vec<IssueRef> = match gate.request.issue.as_deref() {
        Some(issue) => IssueRef::parse(issue).into_iter().collect(),
        None => references(&gate.request.question, "").into_iter().filter(|issue| !issue.repo.is_empty()).collect(),
    };
    !named.is_empty() && named.iter().all(closed)
}

fn closed(issue: &IssueRef) -> bool {
    let open = Mirror::local().fresh(&key(&issue.repo, OPEN_ISSUES), now_ms() as f64 / 1000.0);
    if open.as_ref().and_then(Value::as_array).is_some_and(|nodes| nodes.iter().any(|node| node["number"].as_u64() == Some(issue.number))) {
        return false;
    }
    let repo = format!("{OWNER}/{}", issue.repo);
    crate::platform::github::json(&["issue", "view", &issue.number.to_string(), "-R", &repo, "--json", "state"]).is_ok_and(|found| found["state"] == "CLOSED")
}
