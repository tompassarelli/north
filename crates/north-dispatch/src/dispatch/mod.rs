//! `north fleet dispatch`: keep every free worker slot busy with ready work. Each run reads the hosts'
//! resident sessions, which accounts the out-rule keeps out and the staffed repositories' open issues,
//! launches each ready box it can place through `north fleet launch`, records the launch, and tells
//! Tom in one line when ready work and free slots coexist and nothing could launch.
use crate::error::NorthResult;
use crate::fleet::roster::Roster;
use crate::pacing::placement;
pub use issues::{IssueRef, OWNER, no_box_refusal, product_boxes, references};
pub use decision::{boxless, decide};
use decision::{candidates, decide_leaving, revive};
use plan::Launch;
use survey::Survey;

pub use host_needs::{is_need, register as register_need};
pub use record::cloud_launch_times;
pub use stops::entries_read;
pub use cloud::{Launch as CloudLaunch, Login, autoland as cloud_autoland, branch as cloud_branch, current_login as cloud_login};
pub use fleets::holder as task_holder;
pub use survey::github::recheck as work_gone;
pub use grounds::{BUDGET_TIMES, WINDOW as LEDGER_WINDOW, eta_minutes};
pub use manual::{Ask, Start, begin as start_hand, draw_untasked_window, issue_text, picked_tier, tier_reason, untasked};
pub use pause::{check_passes as pause_check_passes, recorded as recorded_pause, review as review_pause, standing as paused};
pub use record::{Record as LaunchRecord, directory as launch_directory, read as launch_records};
pub use tier::{ARM_WINDOW, Effort, Evidence, FAMILIES, Rung, Tier, evidenced, seed_cell};

mod band;
mod brief;
pub mod cells;
pub mod deepseek;
mod cloud;
mod decision;
pub mod drafts;
pub mod drafting;
mod from_draft;
#[cfg(test)]
mod drafts_tests;
mod effects;
mod facets;
pub use facets::launch_header;
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
mod leads;
mod manual;
mod nearest;
mod pause;
mod queued;
pub mod measure;
mod pack;
mod plan;
mod record;
mod report;
mod retired;
mod revive;
mod screen;
mod scope;
mod slots;
mod spin;
mod stops;
mod strands;
mod survey;
pub use survey::{record_closed_reports, refresh_fleet_listings, spend_now};
mod tier;
pub mod shares;
pub mod tuned;
pub mod waste;
mod tries;
mod work;
mod wait;
#[cfg(test)]
mod backlog_tests;
#[cfg(test)]
mod home_path_tests;
#[cfg(test)]
mod cells_tests;
#[cfg(test)]
mod claim_tests;
#[cfg(test)]
mod threads_claim_tests;
#[cfg(test)]
mod corroborated_tests;
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
mod host_needs_tests;
#[cfg(test)]
mod tries_tests;
#[cfg(test)]
mod spread_tests;
#[cfg(test)]
mod revive_tests;

const USAGE: &str = "usage: north fleet dispatch [--dry-run] [--draft REPO#N...] | review NAME --item REPO#N --category C [--box TEXT] [--landing] | verdict --session NAME --category C --sha SHA clean|defect CLASS | pause --reason TEXT | resume | paused | queued --by HOST --item REPO#N [--box TEXT] -- WORDS | needs REPO#N | need SESSION NEED [REPO#N] (reason on stdin) | route-replay [--days N] [--db PATH]";

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub launches: Vec<Launch>,
    pub skipped: Vec<(IssueRef, String)>,
    /// The issues dispatch found ready to staff, in the order it staffs them.
    pub ready: Vec<IssueRef>,
    pub free: i64,
    pub revival: revive::Revival,
    /// The parents over the lead bound and the leads this run stops.
    pub tree: leads::Tree,
}




/// The first words of a box's text, enough to tell it from its issue's other boxes.
fn box_words(text: &str) -> String {
    text.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]").split_whitespace().take(8).collect::<Vec<_>>().join(" ")
}




/// What one planned launch came to: started, skipped by a guard the run met after planning it, or failed.
enum Started {
    Launched(String),
    Skipped(String),
    Failed(String, String),
}

impl Started {
    fn line(&self) -> &str {
        match self {
            Self::Launched(line) | Self::Skipped(line) | Self::Failed(line, _) => line,
        }
    }

    /// Why the launch did not happen, for the freeze page and Tom's alert.
    fn stopped(&self) -> Option<String> {
        match self {
            Self::Launched(_) => None,
            Self::Skipped(line) => Some(line.clone()),
            Self::Failed(_, alert) => Some(alert.clone()),
        }
    }
}

/// The line one launch prints; a box whose issue GitHub shows closed or fully ticked since
/// the mirror copied it is skipped, and so is every box once the proxy has paused dispatch, even in a run that began before the pause.
fn start(survey: &Survey, launch: &Launch, dry_run: bool, standing: fn() -> Option<String>) -> Started {
    let named = facets::declared_in(&launch.brief, "Box").map_or_else(String::new, |text| format!(" box `{}`", box_words(text)));
    let line = format!("launch {}{named} on {} {} as {}", launch.item, launch.host, launch.account, deepseek::rung(launch));
    if let Some(reason) = standing().filter(|_| !dry_run) {
        return Started::Skipped(format!("skip {}: dispatch was paused while this run was working: {reason}", launch.item));
    }
    if let Some(why) = survey::github::recheck(&launch.item, facets::declared_in(&launch.brief, "Box")) {
        return Started::Skipped(format!("skip {}: {why}", launch.item));
    }
    let noted;
    let launch = match from_draft::at_launch(survey, launch, dry_run) {
        Some(from_draft::FromDraft::Landed(line)) => return Started::Launched(line),
        Some(from_draft::FromDraft::Held(line)) => return Started::Skipped(line),
        Some(from_draft::FromDraft::Note(note)) => {
            noted = Launch { brief: format!("{}{note}", launch.brief), ..launch.clone() };
            &noted
        }
        None => launch,
    };
    if dry_run {
        let why = std::iter::once(launch.route.why.as_str()).chain(launch.local.as_deref()).collect::<Vec<_>>().join("; ");
        return Started::Launched(format!("would {line} ({why})"));
    }
    match effects::launch(survey, launch) {
        Ok(name) => Started::Launched(format!("{line}: {name}")),
        Err(why) => Started::Failed(format!("{line} failed: {why}"), format!("{} on {}: {why}", launch.item, launch.host)),
    }
}

/// Runs the revivals and every launch side by side, since each `north fleet launch` takes most of a minute and the plan already placed them, then prints their lines in plan order; returns why each planned launch that did not happen did not.
fn execute(survey: &Survey, outcome: &Outcome, dry_run: bool) -> Vec<String> {
    let (revived, started) = std::thread::scope(|scope| {
        let revived = scope.spawn(|| revive(survey, &outcome.revival, dry_run, pause::standing));
        let started: Vec<_> = outcome.launches.iter().map(|launch| scope.spawn(move || start(survey, launch, dry_run, pause::standing))).collect();
        let started: Vec<Started> = started.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Started::Failed("a launch thread panicked".to_owned(), "a launch thread panicked".to_owned()))).collect();
        (revived.join().unwrap_or_else(|_| vec!["the revival thread panicked".to_owned()]), started)
    });
    for line in revived.iter().map(String::as_str).chain(started.iter().map(Started::line)) {
        println!("{line}");
    }
    started.iter().filter_map(Started::stopped).collect()
}

fn mode(arguments: &[String]) -> Result<(bool, Vec<IssueRef>), u8> {
    let (dry_run, rest) = match arguments {
        [flag, rest @ ..] if flag == "--dry-run" => (true, rest),
        rest => (false, rest),
    };
    match rest {
        [] => Ok((dry_run, Vec::new())),
        [flag, items @ ..] if flag == "--draft" && !items.is_empty() => draft_items(items).map(|items| (dry_run, items)),
        _ if dry_run => Err(usage_error()),
        _ => Err(verb(arguments)),
    }
}

fn draft_items(items: &[String]) -> Result<Vec<IssueRef>, u8> {
    items.iter().map(|item| IssueRef::parse(item).ok_or(item)).collect::<Result<Vec<_>, _>>().map_err(|item| {
        eprintln!("north fleet dispatch --draft: {item} is no REPO#N");
        2
    })
}

fn usage_error() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn verb(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) => review_verb(verb, rest).or_else(|| queue_verb(verb, rest)).unwrap_or_else(usage_error),
        None => usage_error(),
    }
}

fn review_verb(verb: &str, rest: &[String]) -> Option<u8> {
    match (verb, rest) {
        ("route-replay", _) => Some(nearest::replay(rest)),
        ("pause", _) => Some(pause::pause(rest)),
        ("resume", []) => Some(pause::resume()),
        ("review", _) => Some(deepseek::review::serve(rest)),
        ("verdict", _) => Some(deepseek::review::verdict(rest)),
        _ => None,
    }
}

fn queue_verb(verb: &str, rest: &[String]) -> Option<u8> {
    match (verb, rest) {
        ("paused", []) => Some(queued::paused()),
        ("queued", _) => Some(queued::serve(rest)),
        ("needs", [item]) => Some(host_needs::serve(item)),
        ("need", [session, need, item @ ..]) if item.len() < 2 => Some(host_needs::serve_need(session, need, item.first().map(String::as_str))),
        _ => None,
    }
}

pub fn run(arguments: &[String], roster: fn() -> NorthResult<Roster>) -> u8 {
    let (dry_run, drafted) = match mode(arguments) {
        Ok(asked) => asked,
        Err(code) => return code,
    };
    if !dry_run && let Some(reason) = pause::standing() {
        println!("north fleet dispatch: paused on this host, launching nothing: {reason}; `north fleet dispatch resume` ends the pause");
        return 0;
    }
    if let Some((host, ssh)) = placement::config().ok().and_then(|cfg| home::elsewhere(&cfg, &crate::platform::host::name())) {
        return home::run_there(&crate::platform::host::name(), &host, &ssh, dry_run, &drafted);
    }
    let survey = match roster().map_err(|error| error.to_string()).and_then(|roster| survey::survey(&roster)) {
        Ok(survey) => survey,
        Err(why) => {
            eprintln!("north fleet dispatch: {why}");
            return 2;
        }
    };
    report::print(&survey, dry_run);
    let (outcome, mut pools) = decide_leaving(&survey, &format!("north fleet dispatch on {}", survey.here));
    if !drafted.is_empty() {
        print_lines(drafting::pass(&survey, &outcome, &mut pools, Some(&drafted), dry_run));
        return 0;
    }
    print_plan(&survey, &outcome);
    let boxless = boxless(&survey, &outcome);
    if !boxless.is_empty() {
        println!("needs a box: {}", boxless.iter().map(|issue| issue.reference().to_string()).collect::<Vec<_>>().join(", "));
        effects::ask_for_boxes(&survey.cfg, &survey.listed, &boxless, dry_run, placement::now());
    }
    let stopped = execute(&survey, &outcome, dry_run);
    print_lines(leads::stop_all(&outcome.tree.stops, dry_run, |lead| effects::stop_lead(&survey, lead)));
    print_lines(drafting::pass(&survey, &outcome, &mut pools, None, dry_run));
    print_lines(deepseek::review::queue(&survey.deepseek.review, &survey.here, survey.laptop.as_deref(), &|host, words| effects::north_on(&survey, host, words), dry_run));
    print_lines(survey.deepseek.limit.release(&record::directory(), &mut |host, words| effects::north_on(&survey, host, words), dry_run));
    let now = placement::now();
    let frozen = freeze::frozen(&outcome, &stopped, &freeze::guards(&survey));
    let told = match &frozen {
        Some(basis) => effects::tell_tom(&format!("north fleet dispatch on {}: {basis}", survey.here), dry_run, now),
        None => Ok(()),
    };
    if let Err(why) = &told {
        eprintln!("north fleet dispatch: {why}");
    }
    let reported = effects::report_failures(&record::read(&record::directory()), now, dry_run);
    let watched = freeze::watch(&survey.here, frozen, dry_run, now);
    exit(told, reported, watched)
}

fn print_plan(survey: &Survey, outcome: &Outcome) {
    for (place, item) in outcome.ready.iter().enumerate() {
        let standing = survey.backlog.as_ref().map_or_else(|| "unread graph".to_owned(), |backlog| backlog.standing(&item.to_string()));
        println!("ready {}\t{item}\t{standing}", place + 1);
    }
    print_lines(&outcome.tree.over);
    for (item, why) in &outcome.skipped {
        println!("skip {item}: {why}");
    }
}

fn print_lines(lines: impl IntoIterator<Item = impl std::fmt::Display>) {
    for line in lines {
        println!("{line}");
    }
}

/// The run's exit code: 1 when any of its alert sinks failed, so the fleet tick and the unit's log
/// see a lost alert as a failed dispatch run rather than a successful one (north#1071).
fn exit(told: Result<(), String>, reported: bool, watched: bool) -> u8 {
    u8::from(told.is_err() || !reported || !watched)
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
    cloud::register(&record::directory(), repo, &launched, std::env::var(crate::fleet::runtime::SESSION_ENV).ok().filter(|session| !session.is_empty()).as_deref(), placement::now())?;
    cloud::record_run(&launched, &brief, placement::now());
    if let Some(item) = &item {
        cloud::hold(item, &launched, &crate::platform::host::name(), &record::directory(), placement::now())?;
    }
    Ok(format!("{} {} {}", launched.session, launched.branch, launched.directory.display()))
}
