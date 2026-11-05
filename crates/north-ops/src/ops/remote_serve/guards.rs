use crate::threads::catalog::CATALOG;

use super::{Grants, Jobs, Plan, Refusal};

const POOL: &str = "pool";
const FLEET_ROW_PEERED: [&str; 5] = ["put", "exchange", "register", "wait", "settle"];
const FOLLOW_STEPS: [&str; 4] = ["follow", "follow-release", "follow-started", "follow-alert"];
const GH_STATUSES: [&str; 2] = ["failed", "requeued"];
pub const REMOTE_PEER: &str = "NORTH_REMOTE_PEER";

pub(super) fn guard(mut plan: Plan, grants: &Grants, jobs: &dyn Jobs) -> Result<Plan, Refusal> {
    let refused = |why: String| Refusal::Guard(why);
    let words: Vec<&str> = plan.argv.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["threads", rest @ ..] | ["north", "threads", rest @ ..] => threads(rest, grants, jobs).map_err(refused)?,
        ["north", "fleet", "row", verb, rest @ ..] if FLEET_ROW_PEERED.contains(verb) => {
            match rest {
                [] => {}
                ["--peer", named] if *named == grants.peer => {}
                ["--peer", named] => return Err(refused(format!("fleet row {verb} from {} names --peer {named}", grants.peer))),
                _ => return Err(refused(format!("fleet row {verb} takes only --peer {}", grants.peer))),
            }
            plan.argv.truncate(4);
            plan.argv.extend(["--peer".to_string(), grants.peer.clone()]);
            plan.env.insert(REMOTE_PEER.to_string(), grants.peer.clone());
        }
        _ => {}
    }
    Ok(plan)
}

fn threads(words: &[&str], grants: &Grants, jobs: &dyn Jobs) -> Result<(), String> {
    for word in words {
        repo_granted(word, grants)?;
    }
    let Some((verb, rest)) = words.split_first() else { return Ok(()) };
    match *verb {
        "queue" => queue(rest, grants, jobs),
        "job-run" => job_run(rest, grants),
        "new" => rest.first().map_or(Ok(()), |namespace| granted(namespace, grants)),
        _ => Ok(()),
    }
}

fn repo_granted(word: &str, grants: &Grants) -> Result<(), String> {
    match word.rsplit_once('#') {
        Some((name, number)) if !name.is_empty() && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) => granted(name, grants),
        _ => Ok(()),
    }
}

fn granted(name: &str, grants: &Grants) -> Result<(), String> {
    let repo = CATALOG.canonical(name);
    if grants.repos.iter().any(|granted| CATALOG.canonical(granted) == repo) {
        Ok(())
    } else {
        Err(format!("{name} is not a repository granted to {}", grants.peer))
    }
}

fn queue(words: &[&str], grants: &Grants, jobs: &dyn Jobs) -> Result<(), String> {
    let mut at = 0;
    while let Some(word) = words.get(at).filter(|word| word.starts_with("--")) {
        at += if *word == "--by" { 2 } else { 1 };
    }
    let Some((action, rest)) = words.get(at..).and_then(<[&str]>::split_first) else { return Ok(()) };
    let peer = grants.peer.as_str();
    match *action {
        step if FOLLOW_STEPS.contains(&step) => Err(format!("queue {step} is not served to {peer}")),
        "add" => match rest.first() {
            Some(placement) if *placement == peer => Ok(()),
            placement => Err(format!("queue add from {peer} names placement {}", placement.unwrap_or(&""))),
        },
        "take" => {
            let placements = rest.first().copied().unwrap_or("");
            match placements.split(',').find(|placement| *placement != peer && *placement != POOL) {
                Some(other) => Err(format!("queue take from {peer} names placement {other}")),
                None => Ok(()),
            }
        }
        "msg" | "drop" => {
            let id = rest.first().copied().unwrap_or("");
            let job = id.parse().ok().and_then(|id| jobs.job(id)).ok_or_else(|| format!("queue {action}: job {id} is not in the ledger"))?;
            if job.placement != peer {
                return Err(format!("queue {action}: job {id} is placed on {}, not {peer}", job.placement));
            }
            let reaches_gh = *action == "msg" && rest.get(1).is_some_and(|status| GH_STATUSES.contains(status));
            match job.item.as_deref().filter(|_| reaches_gh) {
                Some(item) => repo_granted(item, grants),
                None => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

fn job_run(words: &[&str], grants: &Grants) -> Result<(), String> {
    let hosts: Vec<&str> = words.windows(2).filter(|pair| pair[0] == "--host").map(|pair| pair[1]).collect();
    match hosts.as_slice() {
        [host] if *host == grants.peer => Ok(()),
        _ => Err(format!("job-run from {} must name --host {} once", grants.peer, grants.peer)),
    }
}
