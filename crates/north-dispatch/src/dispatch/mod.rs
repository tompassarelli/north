//! `north fleet dispatch`: keep every free worker slot busy with ready work. Each run reads the hosts'
//! resident sessions, which accounts the out-rule keeps out and the staffed repositories' open issues,
//! launches each ready box it can place through `north fleet launch`, records the launch, and tells
//! Tom in one line when ready work and free slots coexist and nothing could launch.
use std::collections::{BTreeMap, BTreeSet};
use crate::error::NorthResult;
use crate::fleet::roster::Roster;
use crate::pacing::placement::{self, gate};
use issues::Issue;
use north_graph_ground::Backlog;
pub use issues::{IssueRef, OWNER, product_boxes, references};
use plan::{Candidate, Capacity, Launch, Ready};
use slots::Pools;
use survey::Survey;

pub use host_needs::{is_need, register as register_need};
pub use record::cloud_launch_times;
pub use stops::entries_read;
pub use cloud::{Launch as CloudLaunch, Login, autoland as cloud_autoland, branch as cloud_branch, current_login as cloud_login};
pub use fleets::holder as task_holder;
pub use grounds::{BUDGET_TIMES, WINDOW as LEDGER_WINDOW, eta_minutes};
pub use manual::{Ask, Start, begin as start_hand, picked_tier, tier_reason, untasked};
pub use survey::evidence as arm_evidence;
pub use record::{Record as LaunchRecord, directory as launch_directory, read as launch_records};
pub use tier::{ARM_WINDOW, Effort, Evidence, Tier, evidenced};

mod brief;
mod cloud;
mod effects;
mod facets;
pub(crate) mod fleets;
pub use fleets::workers as fleet_workers;
mod freeze;
#[cfg(test)]
mod freeze_tests;
mod grounds;
mod host_needs;
mod home;
mod hotspots;
mod issues;
mod manual;
mod nearest;
mod pause;
mod queued;
pub mod measure;
mod pack;
mod plan;
mod projects;
mod record;
mod report;
mod revive;
mod screen;
mod slots;
mod stops;
mod strands;
mod survey;
pub use survey::spend_now;
mod tier;
pub mod shares;
pub mod tuned;
pub mod waste;
mod tries;
mod work;
mod wait;
mod wait_stops;
#[cfg(test)]
mod backlog_tests;
#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod threads_claim_tests;
#[cfg(test)]
mod order_tests;
#[cfg(test)]
mod stops_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tier_tests;
#[cfg(test)]
mod grounds_tests;
#[cfg(test)]
mod tries_tests;
#[cfg(test)]
mod spread_tests;
#[cfg(test)]
mod revive_tests;

const USAGE: &str = "usage: north fleet dispatch [--dry-run] | pause --reason TEXT | resume | paused | queued --by HOST --item REPO#N [--box TEXT] -- WORDS | needs REPO#N | need SESSION NEED [REPO#N] (reason on stdin) | route-replay [--days N] [--db PATH]";

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub launches: Vec<Launch>,
    pub skipped: Vec<(IssueRef, String)>,
    /// The issues dispatch found ready to staff, in the order it staffs them.
    pub ready: Vec<IssueRef>,
    pub free: i64,
    pub revival: revive::Revival,
}

/// The issues in the fact graph's measured order (`north graph ready`), each issue with no ready box there after them, by
/// priority label, backlog rank, age, project and number.
fn ordered<'a>(issues: &'a [Issue], projects: &projects::Projects, backlog: Option<&Backlog>) -> Vec<&'a Issue> {
    let leaders = backlog.map(Backlog::leaders).unwrap_or_default();
    let place = |issue: &Issue| leaders.iter().position(|leader| *leader == issue.reference().to_string()).unwrap_or(usize::MAX);
    let rank = |issue: &Issue| backlog.and_then(|backlog| backlog.rank(&issue.reference().to_string())).unwrap_or(i64::MAX);
    let mut ordered: Vec<&Issue> = issues.iter().collect();
    ordered.sort_by(|a, b| {
        place(a)
            .cmp(&place(b))
            .then(a.priority().cmp(&b.priority()))
            .then(rank(a).cmp(&rank(b)))
            .then(a.created.total_cmp(&b.created))
            .then_with(|| projects.position(&a.repo).cmp(&projects.position(&b.repo)))
            .then(a.number.cmp(&b.number))
    });
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

/// The boxes of `issue` that may be staffed now, each running box's paths already held in `claims`; a bare issue is one box with no text.
fn open_boxes(issue: &Issue, running: &BTreeMap<String, String>, tried: &BTreeMap<String, String>, at: f64, claims: &mut hotspots::Claims) -> Vec<Option<String>> {
    for text in issue.takeable(at) {
        if let Some(holder) = crate::threads::claims::box_slug(text).and_then(|slug| running.get(&slug)) {
            claims.take(&hotspots::paths(issue, Some(text)), holder);
        }
    }
    let taken: BTreeSet<String> = running.keys().chain(tried.keys()).cloned().collect();
    let mut boxes: Vec<Option<String>> = issue.free_boxes(&taken, at).map(|text| Some(text.to_owned())).collect();
    if issue.takeable(at).next().is_none() {
        boxes.push(None);
    }
    boxes
}

/// Every ready box placed on a free slot, or skipped with its reason.
pub fn decide(survey: &Survey, launched_by: &str) -> Outcome {
    let mut pools = Pools::new(&survey.hosts, &survey.accounts).with_cloud(cloud::SESSIONS - survey.cloud_running);
    let mut outcome = Outcome { free: pools.total(), ..Outcome::default() };
    let planned = issues::planned_predecessors(&survey.issues);
    let capacity = Capacity { here: &survey.here, cloud: &survey.cloud, accounts: &survey.accounts, charge: survey.rules.charge, spend: &survey.spend, rules: &survey.rules };
    let revival = revive::plan(&revive::Inputs { strands: &survey.strands, saturated: &survey.saturated, revived: &survey.revived }, &capacity, &mut pools);
    let mut holds = survey.holds.clone();
    holds.extend(revival.holds.clone());
    outcome.revival = revival;
    let world = issues::World { open: &survey.open, holds: &holds, box_holds: &survey.box_holds, box_tries: &survey.box_tries, live: &survey.live, planned: &planned, backlog: survey.backlog.as_ref(), at: survey.at };
    let mut claims = hotspots::Claims::default();
    for issue in ordered(&survey.issues, &survey.projects, survey.backlog.as_ref()) {
        if let Err(why) = screen::admitted(survey, issue, &world) {
            outcome.skipped.push((issue.reference(), why));
            continue;
        }
        outcome.ready.push(issue.reference());
        let reference = issue.reference();
        let none = std::collections::BTreeMap::new();
        let running = survey.box_holds.get(&reference).unwrap_or(&none);
        let tried = survey.box_tries.get(&reference).unwrap_or(&none);
        let boxes = open_boxes(issue, running, tried, survey.at, &mut claims);
        let mut staffed = running.len();
        for box_text in &boxes {
            let box_text = box_text.as_deref();
            if issue.worker_cap().is_some_and(|cap| staffed >= cap) {
                break;
            }
            let paths = hotspots::paths(issue, box_text);
            let named = box_text.map_or_else(String::new, |text| format!("box `{text}` "));
            if let Some((path, holder)) = claims.conflict(&paths) {
                outcome.skipped.push((reference.clone(), format!("ready, not placed: {named}{}", hotspots::hold_reason(&path, holder))));
                continue;
            }
            let (route, brief) = match screen::written(survey, issue, launched_by, box_text) {
                Ok(written) => written,
                Err(why) => {
                    outcome.skipped.push((reference.clone(), format!("ready, not placed: {why}")));
                    break;
                }
            };
            let code_only = gate::surface(&brief, &survey.cfg).0 && cloud::autoland(&issue.repo);
            let ready = Ready { item: reference.clone(), brief, code_only, route };
            match plan::place(&ready, &capacity, &candidates(survey, &ready.brief), &mut pools) {
                Ok(mut launch) => {
                    screen::arm(survey, &mut launch);
                    claims.take(&paths, &format!("{named}of {reference}, launched in this run"));
                    outcome.launches.push(launch);
                }
                Err(why) => {
                    outcome.skipped.push((ready.item, format!("ready, not placed: {why}")));
                    break;
                }
            }
            staffed += 1;
        }
    }
    outcome
}

/// The open issues dispatch skipped because they have no Done-when box.
pub fn boxless<'a>(survey: &'a Survey, outcome: &Outcome) -> Vec<&'a Issue> {
    outcome.skipped.iter().filter(|(_, why)| why == issues::NEEDS_BOX).filter_map(|(item, _)| survey.issues.iter().find(|issue| issue.reference() == *item)).collect()
}

/// The lines reviving `revival` prints, in its order.
fn revive(survey: &Survey, revival: &revive::Revival, dry_run: bool, standing: fn() -> Option<String>) -> Vec<String> {
    let mut lines: Vec<String> = revival.waiting.iter().map(|(name, why)| format!("stranded {name} waits: {why}")).collect();
    for step in &revival.steps {
        let line = match step {
            revive::Step::Revive(revive) => format!("revive {} on {} {} ({}; {})", revive.name, revive.host, revive.account, revive.stop.label(), revive.lanes.join(", ")),
            revive::Step::Forget { name, registry } => format!("forget {name} on {registry}: its lanes' work is on origin/main"),
        };
        if dry_run {
            lines.push(format!("would {line}"));
            continue;
        }
        if let (revive::Step::Revive(_), Some(reason)) = (step, standing().filter(|_| !dry_run)) {
            lines.push(format!("skip {line}: dispatch was paused while this run was working: {reason}"));
            continue;
        }
        let done = match step {
            revive::Step::Revive(revive) => effects::revive(survey, revive),
            revive::Step::Forget { name, registry } => effects::forget(survey, name, registry),
        };
        lines.push(match done {
            Ok(()) => line,
            Err(why) => format!("{line} failed: {why}"),
        });
    }
    lines
}

/// The line one launch prints, and its failure for Tom's alert; a box whose issue GitHub shows closed or fully ticked since
/// the mirror copied it is skipped, and so is every box once the proxy has paused dispatch, even in a run that began before the pause.
fn start(survey: &Survey, launch: &Launch, dry_run: bool, standing: fn() -> Option<String>) -> (String, Option<String>) {
    let verb = if launch.cloud { "cloud-launch" } else { "launch" };
    let line = format!("{verb} {} on {} {} as {}", launch.item, launch.host, launch.account, launch.route.tier.name());
    if let Some(reason) = standing().filter(|_| !dry_run) {
        return (format!("skip {}: dispatch was paused while this run was working: {reason}", launch.item), None);
    }
    if let Some(why) = survey::github::recheck(&launch.item, facets::declared_in(&launch.brief, "Box")) {
        return (format!("skip {}: {why}", launch.item), None);
    }
    if dry_run {
        let why = std::iter::once(launch.route.why.as_str()).chain(launch.local.as_deref()).collect::<Vec<_>>().join("; ");
        return (format!("would {line} ({why})"), None);
    }
    match effects::launch(survey, launch) {
        Ok(name) => (format!("{line}: {name}"), None),
        Err(why) => (format!("{line} failed: {why}"), Some(format!("{} on {}: {why}", launch.item, launch.host))),
    }
}

/// Runs the revivals and every launch side by side, since each `north fleet launch` takes most of a minute and the plan already placed them, then prints their lines in plan order.
fn execute(survey: &Survey, outcome: &Outcome, dry_run: bool) -> Vec<String> {
    if !dry_run {
        effects::record_waits(&survey.fresh_waits);
    }
    let (revived, started) = std::thread::scope(|scope| {
        let revived = scope.spawn(|| revive(survey, &outcome.revival, dry_run, pause::standing));
        let started: Vec<_> = outcome.launches.iter().map(|launch| scope.spawn(move || start(survey, launch, dry_run, pause::standing))).collect();
        let started: Vec<(String, Option<String>)> = started.into_iter().map(|handle| handle.join().unwrap_or_else(|_| ("a launch thread panicked".to_owned(), Some("a launch thread panicked".to_owned())))).collect();
        (revived.join().unwrap_or_else(|_| vec!["the revival thread panicked".to_owned()]), started)
    });
    for line in revived.iter().chain(started.iter().map(|(line, _)| line)) {
        println!("{line}");
    }
    started.into_iter().filter_map(|(_, failure)| failure).collect()
}

fn mode(arguments: &[String]) -> Result<bool, u8> {
    match arguments {
        [] => Ok(false),
        [flag] if flag == "--dry-run" => Ok(true),
        [verb, rest @ ..] if verb == "route-replay" => Err(nearest::replay(rest)),
        [verb, rest @ ..] if verb == "pause" => Err(pause::pause(rest)),
        [verb] if verb == "resume" => Err(pause::resume()),
        [verb] if verb == "paused" => Err(queued::paused()),
        [verb, rest @ ..] if verb == "queued" => Err(queued::serve(rest)),
        [verb, item] if verb == "needs" => Err(host_needs::serve(item)),
        [verb, session, need, item @ ..] if verb == "need" && item.len() < 2 => Err(host_needs::serve_need(session, need, item.first().map(String::as_str))),
        _ => {
            eprintln!("{USAGE}");
            Err(2)
        }
    }
}

pub fn run(arguments: &[String], roster: fn() -> NorthResult<Roster>) -> u8 {
    let dry_run = match mode(arguments) {
        Ok(dry_run) => dry_run,
        Err(code) => return code,
    };
    if dry_run && let Some((host, ssh)) = placement::config().ok().and_then(|cfg| home::elsewhere(&cfg, &crate::platform::host::name())) {
        return home::dry_run_there(&crate::platform::host::name(), &host, &ssh);
    }
    if !dry_run && let Some(reason) = pause::standing() {
        println!("north fleet dispatch: paused on this host, launching nothing: {reason}; `north fleet dispatch resume` ends the pause");
        return 0;
    }
    let survey = match roster().map_err(|error| error.to_string()).and_then(|roster| survey::survey(&roster)) {
        Ok(survey) => survey,
        Err(why) => {
            eprintln!("north fleet dispatch: {why}");
            return 2;
        }
    };
    report::print(&survey);
    let outcome = decide(&survey, &format!("north fleet dispatch on {}", survey.here));
    for (place, item) in outcome.ready.iter().enumerate() {
        let standing = survey.backlog.as_ref().map_or_else(|| "unread graph".to_owned(), |backlog| backlog.standing(&item.to_string()));
        println!("ready {}\t{item}\t{standing}", place + 1);
    }
    for (item, why) in &outcome.skipped {
        println!("skip {item}: {why}");
    }
    let boxless = boxless(&survey, &outcome);
    if !boxless.is_empty() {
        println!("needs a box: {}", boxless.iter().map(|issue| issue.reference().to_string()).collect::<Vec<_>>().join(", "));
        effects::ask_for_boxes(&survey.cfg, &survey.listed, &boxless, dry_run, placement::now());
    }
    let failures = execute(&survey, &outcome, dry_run);
    let frozen = freeze::frozen(&outcome, &failures, &freeze::guards(&survey));
    if let Some(basis) = &frozen {
        effects::tell_tom(&format!("north fleet dispatch on {}: {basis}", survey.here), dry_run, placement::now());
    }
    freeze::watch(&survey.here, frozen, dry_run, placement::now());
    0
}

/// `north cloud launch REPO SLUG BRIEF [--item REPO#N]`: starts the brief in the cloud lane, registers
/// it for `north cloud reap`, records it against the item so dispatch holds it, and prints the
/// session, its branch and the checkout.
/// Every session `north cloud launch` started, for `north cloud reap`.
pub fn cloud_launches() -> Vec<CloudLaunch> {
    cloud::launches(&record::directory())
}

pub fn cloud_launch(repo: &str, slug: &str, brief_file: &std::path::Path, item: Option<&str>) -> Result<String, String> {
    let item = item.map(cloud::item).transpose()?;
    let text = std::fs::read_to_string(brief_file).map_err(|error| format!("{}: {error}", brief_file.display()))?;
    let login = cloud::current_login()?;
    let brief = brief::cloud(&text, repo, &cloud::branch(slug), &brief::standing_orders()?);
    let launched = cloud::launch(repo, slug, &brief, &[], std::path::Path::new(cloud::SOURCES), &login.home, &mut cloud::run)?;
    cloud::register(&record::directory(), repo, &launched, placement::now())?;
    cloud::record_run(&launched, &brief, placement::now());
    if let Some(item) = &item {
        cloud::hold(item, &launched, &crate::platform::host::name(), &record::directory(), placement::now())?;
    }
    Ok(format!("{} {} {}", launched.session, launched.branch, launched.directory.display()))
}
