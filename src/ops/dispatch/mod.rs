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
use survey::Survey;

pub(crate) use cloud::{autoland as cloud_autoland, branch as cloud_branch};
pub(crate) use fleets::holder as task_holder;
pub(crate) use grounds::{BUDGET_TIMES, WINDOW as LEDGER_WINDOW, eta_minutes};
pub(crate) use manual::{Ask, Start, begin as start_hand};
pub(crate) use survey::evidence as arm_evidence;
pub(crate) use tier::{ARMED, Evidence, Tier};
#[cfg(test)]
pub(crate) use survey::claim_rows;

mod brief;
mod cloud;
mod effects;
mod facets;
mod fleets;
mod grounds;
mod handoff;
mod issues;
mod manual;
#[cfg(test)]
mod manual_tests;
pub(crate) mod measure;
mod plan;
mod projects;
mod record;
mod slots;
mod stops;
mod survey;
mod tier;
mod tries;
mod wait;
#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod order_tests;
#[cfg(test)]
mod frozen_tests;
#[cfg(test)]
mod stops_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod plan_size_tests;
#[cfg(test)]
mod projects_tests;
#[cfg(test)]
mod tier_tests;
#[cfg(test)]
mod grounds_tests;
#[cfg(test)]
mod tries_tests;
#[cfg(test)]
mod wait_tests;

const USAGE: &str = "usage: north dispatch [--dry-run]";

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Outcome {
    pub launches: Vec<Launch>,
    pub skipped: Vec<(IssueRef, String)>,
    pub ready: usize,
    pub free: i64,
}

fn ordered<'a>(issues: &'a [Issue], projects: &projects::Projects) -> Vec<&'a Issue> {
    let mut ordered: Vec<&Issue> = issues.iter().collect();
    ordered.sort_by_key(|issue| (issue.priority(), projects.position(&issue.repo), issue.number));
    ordered
}

/// The machines a box may run on, soonest expected start first and in `[placement] order` among
/// equals, each with why it refuses this box; a host dispatch cannot measure waits no time.
fn candidates(survey: &Survey, brief: &str) -> Vec<Candidate> {
    let order: Vec<String> = survey.cfg.get("placement").and_then(|rules| rules.get("order")).and_then(toml_edit::Item::as_array).into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
    let class = wait::class(brief);
    let mut ranked: Vec<(f64, usize, Candidate)> = survey
        .hosts
        .iter()
        .map(|host| {
            let waits = host.measure.as_ref().map_or(Ok(0.0), |measure| wait::wait(class, measure));
            let refusal = match (host.free(), gate::lacks(&survey.cfg, &host.name, brief), &waits) {
                (Err(why), _, _) => Some(format!("takes nothing: {why}")),
                (_, Some(need), _) => Some(format!("does not serve {need}")),
                (_, _, Err(why)) => Some(why.clone()),
                _ => None,
            };
            let position = order.iter().position(|name| *name == host.name).unwrap_or(order.len());
            (waits.unwrap_or(f64::INFINITY), position, Candidate { host: host.name.clone(), refusal })
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, candidate)| candidate).collect()
}

/// Every ready box placed on a free slot, or skipped with its reason.
pub(crate) fn decide(survey: &Survey, launched_by: &str) -> Outcome {
    let mut pools = Pools::new(&survey.hosts, &survey.accounts).with_cloud(cloud::SESSIONS - survey.cloud_running);
    let mut outcome = Outcome { free: pools.total(), ..Outcome::default() };
    let planned = issues::planned_predecessors(&survey.issues);
    let world = issues::World { open: &survey.open, holds: &survey.holds, planned: &planned, at: survey.at };
    let mut history = survey.history.clone();
    let mut handoffs = survey.handoffs.clone();
    let capacity = Capacity { here: &survey.here, cloud: &survey.cloud, accounts: &survey.accounts, charge: crate::pace::limits::default("reservation_fallback_points") };
    for issue in ordered(&survey.issues, &survey.projects) {
        if let Some(why) = survey.projects.paused(&issue.repo, survey.at) {
            outcome.skipped.push((issue.reference(), why));
            continue;
        }
        if let Some(topic) = crate::platform::frozen_topics::matching(&survey.frozen, &issue.repo, &issue.title) {
            outcome.skipped.push((issue.reference(), topic.refusal()));
            continue;
        }
        if let Err(why) = issues::readiness(issue, &world) {
            outcome.skipped.push((issue.reference(), why));
            continue;
        }
        outcome.ready += 1;
        let route = tier::route(issue, &history, &survey.ledger);
        let brief = brief::brief(issue, launched_by, &route, &survey.grounds.grounds(&issue.reference().to_string(), &issue.body, &route));
        let code_only = gate::surface(&brief, &survey.cfg).0 && cloud::autoland(&issue.repo);
        let ready = Ready { item: issue.reference(), brief, code_only, route };
        match plan::place(&ready, &capacity, &candidates(survey, &ready.brief), &mut pools) {
            Ok(mut launch) => {
                history.chose(&launch.route);
                if let Some(arm) = (!launch.cloud).then(|| handoffs.pick(launch.route.tier)).flatten() {
                    launch.brief = handoff::armed(&launch.brief, &arm);
                    launch.handoff = Some(arm);
                }
                outcome.launches.push(launch);
            }
            Err(why) => outcome.skipped.push((ready.item, format!("ready, not placed: {why}"))),
        }
    }
    outcome
}

/// The open issues dispatch skipped because they have no Done-when box.
pub(crate) fn boxless<'a>(survey: &'a Survey, outcome: &Outcome) -> Vec<&'a Issue> {
    outcome.skipped.iter().filter(|(_, why)| why == issues::NEEDS_BOX).filter_map(|(item, _)| survey.issues.iter().find(|issue| issue.reference() == *item)).collect()
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
        let line = format!("{verb} {} on {} {} as {}", launch.item, launch.host, launch.account, launch.route.tier.name());
        if dry_run {
            let why = std::iter::once(launch.route.why.as_str()).chain(launch.local.as_deref()).collect::<Vec<_>>().join("; ");
            println!("would {line} ({why})");
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
    let boxless = boxless(&survey, &outcome);
    if !boxless.is_empty() {
        println!("needs a box: {}", boxless.iter().map(|issue| issue.reference().to_string()).collect::<Vec<_>>().join(", "));
        effects::ask_for_boxes(&survey.cfg, &boxless, dry_run, placement::now());
    }
    let failures = execute(&survey, &outcome, dry_run);
    if let Some(line) = alert(&outcome, &failures, &survey.here) {
        effects::tell_tom(&line, dry_run, placement::now());
    }
    0
}

/// `north cloud launch REPO SLUG BRIEF [--item REPO#N]`: starts the brief in the cloud lane, records it
/// against the item so dispatch holds it, and prints the session, its branch and the checkout.
pub(crate) fn cloud_launch(repo: &str, slug: &str, brief_file: &std::path::Path, item: Option<&str>) -> Result<String, String> {
    let item = item.map(cloud::item).transpose()?;
    let text = std::fs::read_to_string(brief_file).map_err(|error| format!("{}: {error}", brief_file.display()))?;
    let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_owned())?;
    let brief = brief::cloud(&text, repo, &cloud::branch(slug), &brief::standing_orders()?);
    let launched = cloud::launch(repo, slug, &brief, &[], std::path::Path::new(cloud::SOURCES), std::path::Path::new(&home), &mut cloud::run)?;
    if let Some(item) = &item {
        cloud::hold(item, &launched, &crate::platform::host::name(), &record::directory(), placement::now())?;
    }
    Ok(format!("{} {} {}", launched.session, launched.branch, launched.directory.display()))
}
