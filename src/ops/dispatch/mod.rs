//! `north dispatch`: keep every free worker slot busy with ready work. Each run reads the hosts'
//! resident sessions, which accounts the out-rule keeps out and the staffed repositories' open issues,
//! launches each ready box it can place through `north fleet launch`, records the launch, and tells
//! Tom in one line when ready work and free slots coexist and nothing could launch.
use crate::error::NorthResult;
use crate::fleet::roster::Roster;
use crate::ops::pacing::placement::{self, gate};
use issues::{Issue, IssueRef};
use plan::{Candidate, Capacity, Launch, Ready};
use slots::Pools;
use survey::{REPOS, Survey};

pub(crate) use cloud::{autoland as cloud_autoland, branch as cloud_branch};

mod brief;
mod cloud;
mod effects;
mod issues;
mod plan;
mod record;
mod slots;
mod survey;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north dispatch [--dry-run]";

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Outcome {
    pub launches: Vec<Launch>,
    pub skipped: Vec<(IssueRef, String)>,
    pub ready: usize,
    pub free: i64,
}

fn ordered(issues: &[Issue]) -> Vec<&Issue> {
    let mut ordered: Vec<&Issue> = issues.iter().collect();
    ordered.sort_by_key(|issue| (issue.priority(), REPOS.iter().position(|repo| *repo == issue.repo), issue.number));
    ordered
}

/// The machines a box may run on, in `[placement] order` and then the rest, each with why it
/// refuses this box.
fn candidates(survey: &Survey, brief: &str) -> Vec<Candidate> {
    let order: Vec<String> = survey.cfg.get("placement").and_then(|rules| rules.get("order")).and_then(toml_edit::Item::as_array).into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
    let mut hosts: Vec<&slots::Host> = survey.hosts.iter().collect();
    hosts.sort_by_key(|host| order.iter().position(|name| *name == host.name).unwrap_or(order.len()));
    hosts
        .into_iter()
        .map(|host| {
            let refusal = match (host.free(), gate::lacks(&survey.cfg, &host.name, brief)) {
                (Err(why), _) => Some(format!("takes nothing: {why}")),
                (_, Some(need)) => Some(format!("does not serve {need}")),
                _ => None,
            };
            Candidate { host: host.name.clone(), refusal }
        })
        .collect()
}

/// Every ready box placed on a free slot, or skipped with its reason.
pub(crate) fn decide(survey: &Survey, launched_by: &str) -> Outcome {
    let mut pools = Pools::new(&survey.hosts, &survey.accounts).with_cloud(cloud::SESSIONS - survey.cloud_running);
    let mut outcome = Outcome { free: pools.total(), ..Outcome::default() };
    let planned = issues::planned_predecessors(&survey.issues);
    let world = issues::World { open: &survey.open, holds: &survey.holds, planned: &planned, at: survey.at };
    let capacity = Capacity { here: &survey.here, cloud: &survey.cloud, accounts: &survey.accounts, charge: crate::pace::limits::default("reservation_fallback_points") };
    for issue in ordered(&survey.issues) {
        if let Err(why) = issues::readiness(issue, &world) {
            outcome.skipped.push((issue.reference(), why));
            continue;
        }
        outcome.ready += 1;
        let brief = brief::brief(issue, launched_by);
        let code_only = gate::surface(&brief, &survey.cfg).0;
        let ready = Ready { item: issue.reference(), brief, code_only };
        match plan::place(&ready, &capacity, &candidates(survey, &ready.brief), &mut pools) {
            Ok(launch) => outcome.launches.push(launch),
            Err(why) => outcome.skipped.push((ready.item, format!("ready, not placed: {why}"))),
        }
    }
    outcome
}

/// The one line Tom gets when ready work and free slots coexist and nothing launched.
pub(crate) fn alert(outcome: &Outcome, failures: &[String], here: &str) -> Option<String> {
    let launched = outcome.launches.len() - failures.len().min(outcome.launches.len());
    if outcome.ready == 0 || outcome.free == 0 || launched > 0 {
        return None;
    }
    let why = failures.first().cloned().or_else(|| outcome.skipped.iter().find(|(_, why)| why.starts_with("ready, not placed")).map(|(item, why)| format!("{item} {why}")));
    Some(format!("north dispatch on {here}: {} ready, {} free slots, none launched: {}", outcome.ready, outcome.free, why.unwrap_or_else(|| "no reason recorded".into())))
}

fn print_survey(survey: &Survey) {
    for host in &survey.hosts {
        println!("{}", host.line());
    }
    for account in &survey.accounts {
        println!("{}", account.line());
    }
    let pools = Pools::new(&survey.hosts, &survey.accounts);
    for host in &survey.hosts {
        for account in survey.accounts.iter().filter(|account| account.hosts.contains(&host.name)) {
            println!("slots {} {}: {}", host.name, account.name, pools.pair(&host.name, &account.name));
        }
    }
    let cloud = match &survey.cloud {
        plan::Cloud::Reachable(line) => format!("reachable here ({line})"),
        plan::Cloud::Unreachable(why) => format!("unreachable from {}: {why}", survey.here),
        plan::Cloud::Closed(line) => format!("closed ({line})"),
    };
    println!("cloud lane: {cloud}");
    for note in &survey.notes {
        println!("note: {note}");
    }
}

fn execute(survey: &Survey, outcome: &Outcome, dry_run: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for launch in &outcome.launches {
        let verb = if launch.cloud { "cloud-launch" } else { "launch" };
        let line = format!("{verb} {} on {} {}", launch.item, launch.host, launch.account);
        if dry_run {
            println!("would {line}{}", launch.local.as_ref().map_or_else(String::new, |why| format!(" ({why})")));
            continue;
        }
        match effects::launch(survey, launch) {
            Ok(name) => println!("{line}: {name}"),
            Err(why) => {
                println!("{line} failed: {why}");
                failures.push(format!("{} on {}: {why}", launch.item, launch.host));
            }
        }
    }
    failures
}

pub(crate) fn run(arguments: &[String], roster: fn() -> NorthResult<Roster>) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let survey = match roster().map_err(|error| error.to_string()).and_then(|roster| survey::survey(&roster)) {
        Ok(survey) => survey,
        Err(why) => {
            eprintln!("north dispatch: {why}");
            return 2;
        }
    };
    print_survey(&survey);
    let outcome = decide(&survey, &format!("north dispatch on {}", survey.here));
    for (item, why) in &outcome.skipped {
        println!("skip {item}: {why}");
    }
    let failures = execute(&survey, &outcome, dry_run);
    if let Some(line) = alert(&outcome, &failures, &survey.here) {
        effects::tell_tom(&line, dry_run, placement::now());
    }
    0
}

/// `north cloud launch REPO SLUG BRIEF`: starts the brief in the cloud lane from this host's login
/// and prints the session, the branch it pushes and the checkout it launched from.
pub(crate) fn cloud_launch(repo: &str, slug: &str, brief_file: &std::path::Path) -> Result<String, String> {
    let text = std::fs::read_to_string(brief_file).map_err(|error| format!("{}: {error}", brief_file.display()))?;
    let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_owned())?;
    let brief = brief::cloud(&text, repo, &cloud::branch(slug));
    let launched = cloud::launch(repo, slug, &brief, std::path::Path::new(cloud::SOURCES), std::path::Path::new(&home), &mut cloud::run)?;
    Ok(format!("{} {} {}", launched.session, launched.branch, launched.directory.display()))
}
