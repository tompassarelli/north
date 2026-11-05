use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::fleet::landings::{self, Push};
use crate::foundation::clock::Timestamp;
use crate::providers::claude::runs::landing::{self, MainIndex};
use crate::providers::claude::runs::north_program;
use crate::threads::catalog::CATALOG;
use crate::threads::runs::landed_issues::Landing;

/// `--landed-issues ACCOUNT --day DATE`: the issues this host's fleet sessions on ACCOUNT landed that day and how
/// many have a done-and-landed run, counted by `north threads landed-issues` on the ledger.
pub(super) fn run(account: &str, day: &str) -> u8 {
    let dated = day.len() == 10 && day.bytes().enumerate().all(|(at, byte)| if matches!(at, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() });
    if account.is_empty() || !dated {
        eprintln!("north worker-ledger: --landed-issues takes an account label and --day YYYY-MM-DD");
        return 2;
    }
    let Some(nodes) = fleet_nodes() else {
        eprintln!("north worker-ledger: north fleet list --json --all is unreadable");
        return 1;
    };
    let main = MainIndex::parse(&super::merged(day));
    let repositories = landing::repositories();
    let pushes = landing::settle(landings::pushes(&landings::push_log()), &main, |push| landing::subject_in(&repositories, push));
    let found = landings(&nodes, &pushes, account, day);
    let spawned = super::threads(&["landed-issues"]).and_then(|mut command| command.stdin(Stdio::piped()).spawn());
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("north worker-ledger: cannot run north threads landed-issues: {error}");
            return 1;
        }
    };
    println!("account {account} on {day}:");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(serde_json::to_string(&found).unwrap_or_default().as_bytes());
    }
    child.wait().ok().and_then(|status| status.code()).map_or(1, |code| u8::try_from(code).unwrap_or(1))
}

fn fleet_nodes() -> Option<Vec<Value>> {
    let output = Command::new(north_program()).args(["fleet", "list", "--json", "--all"]).stderr(Stdio::inherit()).output().ok()?;
    output.status.success().then(|| serde_json::from_slice(&output.stdout).ok()).flatten()
}

/// The landings on `day` (UTC) of the fleet sessions on `account` that name an issue: each push that reached main
/// under a session's name, joined to the issue of that session's record launched before it.
fn landings(nodes: &[Value], pushes: &[Push], account: &str, day: &str) -> Vec<Landing> {
    let on_account = |node: &Value| node["account"].as_str().is_some_and(|label| label == account || label.strip_prefix("claude-") == Some(account));
    let mut found = Vec::new();
    for push in pushes.iter().filter(|push| !push.landed.is_empty() && Timestamp::from_unix_millis(push.at_ms as i64).iso().starts_with(day)) {
        let named: Vec<&Value> = nodes.iter().filter(|node| node["name"] == push.session.as_str() && on_account(node)).collect();
        let launched = |node: &&&Value| node["created_ms"].as_u64().is_none_or(|created| created <= push.at_ms + 1_000);
        let Some(issue) = named.iter().filter(launched).max_by_key(|node| node["created_ms"].as_u64()).and_then(|node| issue_of(&node["task_ref"])) else { continue };
        let conversations = named.iter().filter_map(|node| node["conversation"].as_str().map(str::to_owned)).collect();
        found.push(Landing { issue, session: push.session.clone(), sha: push.landed.clone(), conversations });
    }
    found
}

fn issue_of(task_ref: &Value) -> Option<String> {
    CATALOG.parse_ref(task_ref.as_str()?).ok().map(|(repo, number)| format!("{repo}#{number}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn push(session: &str, landed: &str, at_ms: u64) -> Push {
        Push { at_ms, repository: "north".into(), branch: "lane".into(), oid: "o".into(), landed: landed.into(), session: session.into() }
    }

    #[test]
    fn a_landing_joins_the_issue_of_the_record_launched_before_it_on_the_account_asked() {
        let day = 1_793_836_800_000_u64;
        let nodes = vec![
            json!({"name": "flash-244", "account": "d", "conversation": "old", "task_ref": "wisp#200", "created_ms": day - 50_000}),
            json!({"name": "flash-244", "account": "d", "conversation": "new", "task_ref": "wisp#244", "created_ms": day + 5_000}),
            json!({"name": "flash-244", "account": "d", "conversation": "later", "task_ref": "wisp#999", "created_ms": day + 90_000_000}),
            json!({"name": "other", "account": "b", "conversation": "b-run", "task_ref": "north#1", "created_ms": 0}),
            json!({"name": "loose", "account": "claude-d", "conversation": "loose", "task_ref": null, "created_ms": 0}),
        ];
        let pushes = [push("flash-244", "sha-1", day + 10_000), push("flash-244", "", day + 20_000), push("other", "sha-2", day + 30_000), push("loose", "sha-3", day + 40_000), push("flash-244", "sha-4", day - 10_000)];

        let found = landings(&nodes, &pushes, "d", "2026-11-05");

        assert_eq!(found, [Landing { issue: "wisp#244".into(), session: "flash-244".into(), sha: "sha-1".into(), conversations: vec!["old".into(), "new".into(), "later".into()] }]);
    }
}
