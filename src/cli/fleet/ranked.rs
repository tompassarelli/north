//! A proxy launches a worker only on work the fact graph chooses: an issue among the top five of `north graph ready`'s
//! measured order, or one it created or edited in a turn answering a message Tom typed (north#734). The rule is the
//! `proxy-launch-rank` gate of harness/gates.toml; under shadow it allows the launch, warns the proxy and logs what it
//! would refuse.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::dispatch::IssueRef;
use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::TypedTurn;
use crate::fleet::model::{LaunchRequest, Role};
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;

use super::lineage;
use super::typed::typed_turn;

const GATE: &str = "proxy-launch-rank";
const TOP: usize = 5;

pub(super) fn admit(fleet: &Fleet, request: &LaunchRequest, caller: Option<&str>) -> NorthResult<()> {
    if request.verification || request.role() != Role::Worker || !lineage::from_proxy(fleet, caller) {
        return Ok(());
    }
    let backlog = match north_graph_ground::read_backlog() {
        Ok(backlog) => backlog,
        Err(why) => {
            eprintln!("fleet launch: the fact graph is unread, so {} is not checked against its ranking: {why}", request.name);
            return Ok(());
        }
    };
    let leaders: Vec<(String, String)> = backlog.leaders().into_iter().take(TOP).map(|issue| (issue.to_owned(), backlog.standing(issue))).collect();
    let issues: Vec<IssueRef> = request.tasks().iter().filter_map(|task| IssueRef::parse(task)).collect();
    let turn = caller.and_then(|caller| typed_turn(fleet, caller));
    let Err(refusal) = verdict(&issues, &leaders, turn.as_ref(), edited_ms) else { return Ok(()) };
    if enforced() {
        return Err(NorthError::Refused(refusal));
    }
    eprintln!("fleet launch: {GATE} (shadow) would refuse {}: {refusal}", request.name);
    shadow_row(&json!({"gate": GATE, "at": Timestamp::now().iso(), "caller": caller, "session": request.name, "item": request.item_line(), "would_refuse": refusal}));
    Ok(())
}

/// Ok when the launch names no issue (a worker that files its own, a red main to repair), its issues include one of `leaders` (issue and standing, best first), or every one was created or
/// edited at or after a typed `turn` began; else the refusal naming the top of the ranking.
pub(super) fn verdict(issues: &[IssueRef], leaders: &[(String, String)], turn: Option<&TypedTurn>, edited: impl Fn(&IssueRef) -> Option<u64>) -> Result<(), String> {
    if issues.is_empty() || issues.iter().any(|issue| leaders.iter().any(|(leader, _)| *leader == issue.to_string())) {
        return Ok(());
    }
    if let Some(turn) = turn
        && issues.iter().all(|issue| edited(issue).is_some_and(|at| at >= turn.at_ms))
    {
        return Ok(());
    }
    let named = format!("names {}", issues.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
    let typed = match turn {
        Some(_) => "was not created or edited in this turn, which answers a message Tom typed",
        None => "and this turn answers no message Tom typed",
    };
    let top = if leaders.is_empty() { "the fact graph has no ready box".to_owned() } else { leaders.iter().enumerate().map(|(place, (issue, standing))| format!("{}. {issue} ({standing})", place + 1)).collect::<Vec<_>>().join("; ") };
    Err(format!(
        "a proxy launches a worker only on one of the top {TOP} issues of `north graph ready` or an issue it filed or edited for a request Tom typed; this launch's Item {named}, outside the top {TOP}, {typed}. Top {TOP}: {top}. Let dispatch staff the ranked work, or file what Tom asked as an issue in the turn that answers him"
    ))
}

fn enforced() -> bool {
    crate::platform::harness::document("harness/gates.toml")["gate"][GATE]["mode"].as_str() == Some("enforce")
}

fn shadow_row(row: &Value) {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from);
    let directory = state.join("north/gates");
    let written = fs::create_dir_all(&directory).and_then(|()| OpenOptions::new().create(true).append(true).open(directory.join(format!("{GATE}.jsonl")))).and_then(|mut file| writeln!(file, "{row}"));
    if let Err(why) = written {
        eprintln!("fleet launch: {GATE}'s shadow row was not written: {why}");
    }
}

/// When the issue was created or its body last edited, from the GitHub mirror.
fn edited_ms(issue: &IssueRef) -> Option<u64> {
    use north_github_mirror::store::{Mirror, OPEN_ISSUES, key};
    let copy = Mirror::local().read(&key(&issue.repo, OPEN_ISSUES))?;
    let node = copy.body.as_array()?.iter().find(|node| node["number"].as_u64() == Some(issue.number))?.clone();
    ["createdAt", "lastEditedAt"].iter().filter_map(|field| Timestamp::parse_rfc3339(node[*field].as_str()?)).map(|at| at.unix_seconds_f64()).fold(None, |latest: Option<f64>, at| Some(latest.map_or(at, |latest| latest.max(at)))).map(|seconds| (seconds * 1000.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::fleet::typed::typed;

    fn leaders() -> Vec<(String, String)> {
        ["north#706", "north#638", "north#634", "north#360", "north#636"].iter().map(|issue| ((*issue).to_owned(), "1.0 = $1.00 lost 24 h / $1.00 a box, rank 1".to_owned())).collect()
    }

    fn issue(number: u64) -> Vec<IssueRef> {
        vec![IssueRef::new("north", number)]
    }

    #[test]
    fn a_proxy_launch_off_the_top_five_is_refused_unless_it_files_what_tom_typed_in_that_turn() {
        let never = |_: &IssueRef| None;
        assert_eq!(verdict(&issue(634), &leaders(), None, never), Ok(()), "a top-five issue launches");
        assert_eq!(verdict(&[], &leaders(), None, never), Ok(()), "a launch that files its own issue or repairs a red main names nothing to rank");
        let refused = verdict(&issue(88), &leaders(), None, never).unwrap_err();
        assert!(refused.contains("names north#88") && refused.contains("answers no message Tom typed") && refused.contains("5. north#636 ("), "{refused}");
        let turn = TypedTurn { conversation: "c".into(), at_ms: 1_000 };
        let turn = Some(&turn);
        assert_eq!(verdict(&issue(740), &leaders(), turn, |_: &IssueRef| Some(1_500)), Ok(()), "an issue filed in a turn answering Tom launches");
        let stale = verdict(&issue(88), &leaders(), turn, |_: &IssueRef| Some(500)).unwrap_err();
        assert!(stale.contains("was not created or edited in this turn"), "{stale}");
        assert!(typed("Launch the brief at /tmp/brief.md") && !typed("[from n-frontier] Done: landed") && !typed("<task-notification>"));
    }
}
