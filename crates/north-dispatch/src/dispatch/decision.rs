use std::collections::{BTreeMap, BTreeSet};

use super::issues::{self, Issue};
use super::leads::{self, Tree};
use super::plan::{self, Candidate, Capacity, Launch, Lineage, Ready};
use super::scope::Scope;
use super::slots::Pools;
use super::survey::Survey;
use super::{IssueRef, Outcome, band, box_words, cloud, deepseek, effects, hotspots, screen, wait};
use crate::pacing::placement::gate;
use north_graph_ground::Backlog;

/// Every ready box placed on a free slot, or skipped with its reason.
pub fn decide(survey: &Survey, launched_by: &str) -> Outcome {
    decide_leaving(survey, launched_by).0
}

/// The open issues dispatch skipped because they have no Done-when box.
pub fn boxless<'a>(survey: &'a Survey, outcome: &Outcome) -> Vec<&'a Issue> {
    outcome.skipped.iter().filter(|(_, why)| why == issues::NEEDS_BOX).filter_map(|(item, _)| survey.issues.iter().find(|issue| issue.reference() == *item)).collect()
}

/// The lines reviving `revival` prints, in its order.
pub(super) fn revive(survey: &Survey, revival: &super::revive::Revival, dry_run: bool, standing: fn() -> Option<String>) -> Vec<String> {
    let mut lines: Vec<String> = revival.waiting.iter().map(|(name, why)| format!("stranded {name} waits: {why}")).collect();
    lines.extend(revival.steps.iter().map(|step| revive_step(survey, step, dry_run, standing)));
    lines
}

fn revive_step(survey: &Survey, step: &super::revive::Step, dry_run: bool, standing: fn() -> Option<String>) -> String {
    let line = match step {
        super::revive::Step::Revive(revive) => format!("revive {} on {} {} ({}; {})", revive.name, revive.host, revive.account, revive.stop.label(), revive.lanes.join(", ")),
        super::revive::Step::Forget { name, registry } => format!("forget {name} on {registry}: its lanes' work is on origin/main"),
    };
    if dry_run {
        return format!("would {line}");
    }
    let paused = standing();
    if let (super::revive::Step::Revive(_), Some(reason)) = (step, paused) {
        return format!("skip {line}: dispatch was paused while this run was working: {reason}");
    }
    let done = match step {
        super::revive::Step::Revive(revive) => effects::revive(survey, revive),
        super::revive::Step::Forget { name, registry } => effects::forget(survey, name, registry),
    };
    match done {
        Ok(()) => line,
        Err(why) => format!("{line} failed: {why}"),
    }
}

pub(super) fn decide_leaving(survey: &Survey, launched_by: &str) -> (Outcome, Pools) {
    let (outcome, pools, deferred) = place_all(survey, launched_by, true);
    if deferred && outcome.launches.is_empty() {
        let (outcome, pools, _) = place_all(survey, launched_by, false);
        return (outcome, pools);
    }
    (outcome, pools)
}

/// One placing pass; `hold_predicted` makes a box that overlaps only a running session's predicted writes wait, and the last value says whether one did.
fn place_all(survey: &Survey, launched_by: &str, hold_predicted: bool) -> (Outcome, Pools, bool) {
    let scope = Scope::parse(&survey.cfg);
    let mut staffing = Staffing::new(survey);
    if hold_predicted {
        for (holder, paths) in &survey.predicted {
            staffing.claims.take_predicted(paths, holder);
        }
    }
    let planned = issues::planned_predecessors(&survey.issues);
    let capacity = Capacity { accounts: &survey.accounts, charge: survey.rules.charge, ceiling: survey.rules.money_ceiling, claude_sessions: survey.rules.claude_sessions, spend: &survey.spend, scope: &scope };
    let revival = super::revive::plan(&super::revive::Inputs { strands: &survey.strands, saturated: &survey.saturated, revived: &survey.revived }, &capacity, &mut staffing.pools);
    let mut holds = survey.holds.clone();
    holds.extend(revival.holds.clone());
    staffing.outcome.revival = revival;
    let (box_holds, stale) = issues::settle_box_holds(&survey.issues, &survey.box_holds);
    staffing.outcome.skipped.extend(stale);
    let world = issues::World { open: &survey.open, holds: &holds, box_holds: &box_holds, box_tries: &survey.box_tries, live: &survey.live, planned: &planned, backlog: survey.backlog.as_ref(), at: survey.at };
    let inputs = leads::Inputs { issues: &survey.issues, open: &survey.open, sessions: &survey.sessions, holds: &holds, box_holds: &box_holds, at: survey.at };
    let tree = Tree::read(&inputs, |issue| screen::admitted(survey, issue, &world).is_ok(), leads::Bounds::configured());
    let round = Round { survey, scope: &scope, capacity: &capacity, world: &world, launched_by, tree: &tree };
    stage_leads(&round, &mut staffing);
    for issue in ordered(&survey.issues, &survey.projects, survey.backlog.as_ref()) {
        stage_issue(&round, issue, &mut staffing);
    }
    staffing.outcome.tree = tree;
    (staffing.outcome, staffing.pools, staffing.deferred)
}

struct Staffing {
    outcome: Outcome,
    pools: Pools,
    claims: hotspots::Claims,
    deferred: bool,
    /// The parents whose lead this run launches, which it staffs no box of.
    led: BTreeSet<IssueRef>,
}

impl Staffing {
    fn new(survey: &Survey) -> Self {
        let pools = Pools::new(&survey.hosts, &survey.accounts).held_by_line(survey.line_held.clone());
        let outcome = Outcome { free: pools.total(), ..Outcome::default() };
        Self { outcome, pools, claims: hotspots::Claims::default(), deferred: false, led: BTreeSet::new() }
    }
}

struct Round<'a> {
    survey: &'a Survey,
    scope: &'a Scope,
    capacity: &'a Capacity<'a>,
    world: &'a issues::World<'a>,
    launched_by: &'a str,
    tree: &'a Tree,
}

enum Placed {
    Launched,
    Passed,
    Stopped,
}

fn stage_issue(round: &Round, issue: &Issue, staffing: &mut Staffing) {
    if staffing.led.contains(&issue.reference()) {
        staffing.outcome.skipped.push((issue.reference(), format!("its lead {} launches in this run", leads::name(&issue.reference()))));
        return;
    }
    if let Err(why) = screen::admitted(round.survey, issue, round.world) {
        staffing.outcome.skipped.push((issue.reference(), why));
        return;
    }
    let reference = issue.reference();
    staffing.outcome.ready.push(reference.clone());
    let none = BTreeMap::new();
    let running = round.world.box_holds.get(&reference).unwrap_or(&none);
    let tried = round.survey.box_tries.get(&reference).unwrap_or(&none);
    let boxes = open_boxes(issue, running, tried, round.survey.at, &mut staffing.claims);
    for text in issue.takeable(round.survey.at) {
        if let Some(holder) = crate::threads::claims::box_slug(text).and_then(|slug| running.get(&slug)) {
            staffing.outcome.skipped.push((reference.clone(), format!("box `{}` held: {holder}", box_words(text))));
        }
    }
    let boxes = round.survey.mainline.unretired(issue, boxes, &mut staffing.outcome.skipped);
    let mut staffed = running.len();
    for box_text in &boxes {
        if issue.worker_cap().is_some_and(|cap| staffed >= cap) {
            break;
        }
        match place_box(round, issue, &reference, box_text.as_deref(), staffing) {
            Placed::Launched => staffed += 1,
            Placed::Passed => {}
            Placed::Stopped => break,
        }
    }
}

fn place_box(round: &Round, issue: &Issue, reference: &IssueRef, box_text: Option<&str>, staffing: &mut Staffing) -> Placed {
    let paths = hotspots::paths(issue, box_text);
    let named = box_text.map_or_else(String::new, |text| format!("box `{text}` "));
    if let Some((path, holder)) = staffing.claims.conflict(&paths) {
        staffing.outcome.skipped.push((reference.clone(), format!("ready, not placed: {named}{}", hotspots::hold_reason(&path, holder))));
        return Placed::Passed;
    }
    if let Some((path, holder)) = staffing.claims.predicted_conflict(&paths) {
        staffing.deferred = true;
        staffing.outcome.skipped.push((reference.clone(), format!("ready, not placed: {named}{}", hotspots::predicted_reason(&path, holder))));
        return Placed::Passed;
    }
    let (route, brief) = match screen::written(round.survey, issue, round.launched_by, box_text) {
        Ok(written) => written,
        Err(why) => {
            staffing.outcome.skipped.push((reference.clone(), format!("ready, not placed: {why}")));
            return Placed::Stopped;
        }
    };
    let code_only = gate::surface(&brief, &round.survey.cfg).0 && cloud::autoland(&issue.repo);
    let ready = Ready { item: reference.clone(), brief, code_only, route };
    let candidates = candidates(round.survey, round.scope, &ready.brief);
    match staff(round, issue, box_text, &ready, &candidates, &mut staffing.pools) {
        Ok(mut launch) => {
            screen::arm(round.survey, &mut launch);
            launch.lineage.parent = round.tree.reader(reference);
            staffing.claims.take(&paths, &format!("{named}of {reference}, launched in this run"));
            staffing.outcome.launches.push(launch);
            Placed::Launched
        }
        Err(why) => {
            staffing.outcome.skipped.push((ready.item, format!("ready, not placed: {why}")));
            Placed::Stopped
        }
    }
}

fn staff(round: &Round, issue: &Issue, box_text: Option<&str>, ready: &Ready, candidates: &[Candidate], pools: &mut Pools) -> Result<Launch, String> {
    let scope = round.scope;
    let claude = |pools: &mut Pools| if scope.allows_provider("claude") { plan::place(ready, round.capacity, candidates, pools) } else { Err(scope.why_not_provider("claude")) };
    match (scope.allows_provider(deepseek::PROVIDER), scope.allows_provider("claude")) {
        (false, _) => claude(pools),
        (true, true) => round.survey.deepseek.staff(issue, box_text, ready, candidates, pools, claude),
        (true, false) => match band::outside(issue, &ready.route.category, &round.survey.cfg) {
            Some(why) => Err(format!("{}; {why}", scope.why_not_provider("claude"))),
            None => round.survey.deepseek.takes(issue, box_text).and_then(|()| round.survey.deepseek.place(ready, candidates, pools)).map_err(|why| format!("DeepSeek does not take it: {why}; {}", scope.why_not_provider("claude"))),
        },
    }
}

/// The issues in the fact graph's measured order (`north graph ready`), each issue with no ready box there after them, by
/// priority label, backlog rank, age, project and number; an issue the survey read twice is staffed once, so a run never
/// launches a box it already launched.
fn ordered<'a>(issues: &'a [Issue], projects: &crate::threads::projects::Projects, backlog: Option<&Backlog>) -> Vec<&'a Issue> {
    let leaders = backlog.map(Backlog::leaders).unwrap_or_default();
    let place = |issue: &Issue| leaders.iter().position(|leader| *leader == issue.reference().to_string()).unwrap_or(usize::MAX);
    let rank = |issue: &Issue| backlog.and_then(|backlog| backlog.rank(&issue.reference().to_string())).unwrap_or(i64::MAX);
    let mut seen = BTreeSet::new();
    let mut ordered: Vec<&Issue> = issues.iter().filter(|issue| seen.insert(issue.reference())).collect();
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
pub(super) fn candidates(survey: &Survey, scope: &Scope, brief: &str) -> Vec<Candidate> {
    let order: Vec<String> = survey.cfg.get("placement").and_then(|rules| rules.get("order")).and_then(toml_edit::Item::as_array).into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
    let class = wait::class(brief);
    let unreadable = survey.home.as_deref().map(|home| crate::platform::home_paths::home_paths(brief, home)).unwrap_or_default();
    let mut ranked: Vec<(f64, usize, Candidate)> = survey
        .hosts
        .iter()
        .map(|host| {
            let waits = host.measure.as_ref().map_or(Ok(0.0), |measure| wait::wait(class, measure));
            let apart = host.name != survey.here && survey.laptop.as_deref() != Some(host.name.as_str());
            let refusal = match (host.free(), gate::lacks(&survey.cfg, &host.name, brief), &waits) {
                _ if scope.excludes_host(&host.name) => Some(scope.why_not_host(&host.name)),
                (Err(why), _, _) => Some(format!("takes nothing: {why}")),
                (_, Some(need), _) => Some(format!("does not serve {need}")),
                (_, _, Err(why)) => Some(why.clone()),
                _ if apart && !unreadable.is_empty() => Some(format!("cannot read {}, under {}'s home", unreadable.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "), survey.here)),
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

/// Places one lead for each parent over the bound, the largest load first, while `dispatch_leads` leaves room; a lead
/// takes its slot, cap and spend admission through `plan::place` as a box does.
fn stage_leads(round: &Round, staffing: &mut Staffing) {
    let mut room = round.tree.room;
    for over in &round.tree.over {
        if room == 0 {
            break;
        }
        let Some(issue) = round.survey.issues.iter().find(|issue| issue.reference() == over.item) else { continue };
        match place_lead(round, issue, over, &mut staffing.pools) {
            Ok(launch) => {
                room -= 1;
                staffing.led.insert(over.item.clone());
                staffing.outcome.launches.push(launch);
            }
            Err(why) => staffing.outcome.skipped.push((over.item.clone(), format!("lead not placed: {why}"))),
        }
    }
}

fn place_lead(round: &Round, issue: &Issue, over: &leads::Over, pools: &mut Pools) -> Result<Launch, String> {
    screen::parked(round.survey, issue)?;
    if let Some(why) = issue.handed_off() {
        return Err(why);
    }
    if !round.scope.allows_provider("claude") {
        return Err(round.scope.why_not_provider("claude"));
    }
    let route = super::tier::route_for(&over.item.to_string(), &|key: &str| if key == "Category" { leads::CATEGORY.to_owned() } else { super::facets::facet(issue, key) }, &round.survey.history, &round.survey.ledger)?;
    let (route, written) = screen::written_as(round.survey, issue, round.launched_by, None, route)?;
    let ready = Ready { item: over.item.clone(), brief: leads::brief(&written, over, round.launched_by), code_only: false, route };
    let candidates = candidates(round.survey, round.scope, &ready.brief);
    let mut launch = plan::place(&ready, round.capacity, &candidates, pools)?;
    launch.lineage = Lineage { lead: true, parent: round.tree.reader_above(&over.item) };
    Ok(launch)
}
