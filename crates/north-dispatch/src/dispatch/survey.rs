//! The live inputs one dispatch run reads: South's orchestration, each account's usage sample and
//! the spend rule's view of them, each host's resident sessions, fleet roster and open gates,
//! NorthThreads claims, the dispatch record and the open issues of the staffed repositories.
use super::issues::{self, Issue, IssueRef, OWNER};
use super::plan::Cloud;
use super::record;
use super::slots::{Account, Host, PlanSize};
use super::projects::Projects;
use super::tier::{self, Evidence, History};
use super::grounds::Ledger;
use super::fleets::{self, PROBE, Session, remote_json};
use super::revive::Strand;
use crate::error::NorthResult;
use crate::fleet::model::ProviderName;
use crate::fleet::records::RetiredReport;
use crate::ledger::ClosedReport;
use crate::ledger::closed::ArmBox;
use crate::fleet::roster::{Roster, Standing};
use crate::pacing::placement::{self, gate};
use crate::pacing::placement::case::{grants_left, keyed_here};
use north_strategy_types::placement::GrantSource;
use crate::platform::process::run_within;
use crate::spend::{self, Rules, View};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item};

pub(super) use crate::pacing::placement::room::room;
use crate::pacing::placement::room::{Basis, Rank, Room};

pub struct Survey {
    pub here: String,
    pub cfg: DocumentMut,
    pub projects: Projects,
    pub hosts: Vec<Host>,
    pub accounts: Vec<Account>,
    /// What the spend rule reads: every account's readings, the active worker sessions and the trips.
    pub spend: View,
    pub rules: Rules,
    pub issues: Vec<Issue>,
    pub open: BTreeSet<IssueRef>,
    pub holds: BTreeMap<IssueRef, String>,
    /// The boxes workers hold, by box slug; they hold a box, not the issue.
    pub box_holds: record::BoxHolds,
    /// The open boxes dispatch tried twice in a day without ticking, by box slug, which wait for the proxy and count toward no cap.
    pub box_tries: record::BoxHolds,
    /// The fleet sessions every reached registry lists as running or parked on a wait, which own the boxes their names are on.
    pub live: BTreeSet<String>,
    /// Every session name a reached registry lists, parked or stopped included.
    pub listed: BTreeSet<String>,
    pub cloud: Cloud,
    /// The host reached through the laptop job queue, when one serves `laptop`.
    pub laptop: Option<String>,
    /// Cloud sessions dispatch started whose boxes are still open.
    pub cloud_running: i64,
    pub notes: Vec<String>,
    pub frozen: Vec<crate::platform::frozen_topics::Topic>,
    /// Dispatch's earlier launches by tier, which a Haiku retry and the next arm read.
    pub history: History,
    /// `north ledger worker --recommend` for each armed category.
    pub ledger: BTreeMap<String, Evidence>,
    /// The closed boxes and prior reports each brief grounds its ETA, budget and trace in.
    pub grounds: Ledger,
    /// Stopped sessions holding unlanded lanes, which dispatch revives before it launches.
    pub strands: Vec<Strand>,
    /// The saturation refusal of each host a strand lives on, or why it is unread.
    pub saturated: BTreeMap<String, String>,
    /// The report each session was last revived for, by `ended_ms`.
    pub revived: BTreeMap<String, u64>,
    /// The box waits and ranks in the fact graph, absent when the graph could not be read.
    pub backlog: Option<north_graph_ground::Backlog>,
    /// The dated stops this pass learned for the first time, which `execute` records as waits causes.
    pub fresh_waits: Vec<super::wait_stops::Learned>,
    /// The DeepSeek cap, the sessions holding it, the boxes DeepSeek tried and the sessions that reported Done.
    pub deepseek: super::deepseek::Seen,
    pub at: f64,
}


fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn host_table<'a>(cfg: &'a DocumentMut, name: &str) -> Option<&'a Item> {
    cfg.get("hosts")?.get(name)
}

/// The issues the open gates in one host's `north gate list --json` name, by `--issue`, in the
/// question or by the asking session's name, each with the gate that holds it.
pub fn gate_rows(listing: &Value, host: &str) -> Vec<(IssueRef, String)> {
    let mut found = Vec::new();
    for gate in listing.as_array().into_iter().flatten().filter(|gate| gate["closed_ms"].is_null()) {
        let session = gate["session"].as_str().unwrap_or_default();
        let repo = fleets::named_items(session).first().map_or_else(String::new, |item| item.repo.clone());
        let question = gate["question"].as_str().unwrap_or_default();
        let held_on = gate["host"].as_str().unwrap_or(host);
        let label = gate["id"].as_str().map_or_else(|| format!("{held_on}:{}", gate["id"]), str::to_owned);
        let why = format!("open north gate {label} on {held_on} from {session}, for Tom");
        let mut items: Vec<IssueRef> = issues::references(question, &repo).into_iter().filter(|item| !item.repo.is_empty()).chain(fleets::named_items(session)).chain(gate["issue"].as_str().and_then(IssueRef::parse)).collect();
        items.sort();
        items.dedup();
        found.extend(items.into_iter().map(|item| (item, why.clone())));
    }
    found
}

fn host(cfg: &DocumentMut, name: &str, here: &str, hosted_elsewhere: i64, laptop_running: i64) -> Host {
    let table = host_table(cfg, name);
    let cap = table.and_then(|host| host.get("resident_cap")).and_then(Item::as_integer);
    let ssh = table.and_then(|host| host.get("ssh")).and_then(Item::as_str);
    let laptop = strings(table.and_then(|host| host.get("serves"))).iter().any(|need| need == "laptop");
    let running = if name == here {
        placement::resident_clis(name, "").ok_or_else(|| "resident count unreadable".to_owned())
    } else if let Some(ssh) = ssh {
        placement::resident_clis(name, ssh).map(|live| live + hosted_elsewhere).ok_or_else(|| format!("unreachable over ssh {ssh}"))
    } else if laptop {
        match placement::laptop_state().as_str() {
            "available" => Ok(laptop_running),
            state => Err(format!("laptop {state}")),
        }
    } else {
        Err("no ssh destination".into())
    };
    Host { name: name.into(), cap, running, measure: super::measure::read(name == here, ssh), builds: None }.probed(here, ssh)
}

fn account_hosts(cfg: &DocumentMut, account: &str, here: &str) -> Vec<String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let profiles = cfg.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|policy| policy.get("profiles")).and_then(|profiles| profiles.get(label));
    match profiles.and_then(Item::as_table_like) {
        Some(hosts) => hosts.iter().map(|(name, _)| name.to_owned()).collect(),
        None => vec![here.to_owned()],
    }
}

/// The hosts an account runs on: those South signs it in on, less this host when this host's
/// roster has no login of its own for it.
pub fn signed_in(cfg: &DocumentMut, roster: &Roster, account: &str, here: &str) -> Vec<String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let own = matches!(roster.standing(&ProviderName::new(provider), label), Standing::Listed(_));
    account_hosts(cfg, account, here).into_iter().filter(|host| own || host != here).collect()
}

/// One account as dispatch orders it: its plan size and the room its usage sample leaves.
pub fn account(name: &str, usage: &Value, basis: &Basis, hosts: Vec<String>, at: f64, notes: &mut Vec<String>) -> Account {
    let plan = basis.plan(name).unwrap_or_else(|why| {
        notes.push(why);
        PlanSize { five_hour: 1.0, week: 1.0 }
    });
    Account { name: name.to_owned(), hosts, plan, room: room(usage, at), grant: None }
}

fn accounts(cfg: &DocumentMut, roster: &Roster, here: &str, at: f64, notes: &mut Vec<String>) -> Vec<Account> {
    let basis = Basis::new(cfg, at);
    let usage = placement::pace_state("usage-gate.json");
    let windows = &usage["claude"]["windows"];
    let mut found = Vec::new();
    for (name, sample) in windows.as_object().into_iter().flatten().filter(|(name, _)| name.starts_with("claude:")) {
        match basis.held(name, windows) {
            Some(why) => notes.push(format!("{name} {why}")),
            None => found.push(account(name, sample, &basis, signed_in(cfg, roster, name, here), at, notes)),
        }
    }
    found.extend(grants(cfg, roster, here, grants_left(at)));
    found
}

/// Each API grant in `sources` as an account on the hosts the policy gives its key, ranked by its deadline;
/// this host counts only when its own roster holds the key, so a dispatcher with no key still places on the hosts that do.
pub(super) fn grants(cfg: &DocumentMut, roster: &Roster, here: &str, sources: Vec<GrantSource>) -> Vec<Account> {
    let orgs = cfg.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_orgs"));
    sources
        .into_iter()
        .filter_map(|source| {
            let name = source.account.trim_start_matches("claude:api:");
            let keys = orgs.and_then(|orgs| orgs.get(name)).and_then(|org| org.get("keys")).and_then(Item::as_table_like);
            let here_keyed = keyed_here(roster, &source.account);
            let hosts: Vec<String> = keys.into_iter().flat_map(|keys| keys.iter().map(|(host, _)| host.to_owned())).filter(|host| host != here || here_keyed).collect();
            let room = Room { five_hour: 100.0, week: 100.0, week_reset_h: source.deadline_h };
            (!hosts.is_empty()).then(|| Account { name: source.account, hosts, plan: PlanSize { five_hour: 1.0, week: 1.0 }, room, grant: Some(Rank::grant(source.deadline_h, source.urgent)) })
        })
        .collect()
}

/// The spend rule's view at `at`: every account's readings, counted while some host is signed in to
/// it, and the worker sessions active in `sessions`.
fn spend_view(cfg: &DocumentMut, roster: &Roster, here: &str, rules: &Rules, sessions: &[(Session, String)], at: f64) -> View {
    spend::view_from_config(cfg, &placement::pace_directory(), rules, fleets::active_workers(sessions), at, |name| !signed_in(cfg, roster, name, here).is_empty())
}

/// The spend view as a dispatch survey reads it, for `north fleet launch`: every reached host's
/// active worker sessions, with the rules it applies.
pub fn spend_now(roster: &Roster) -> Result<(View, Rules), String> {
    let cfg = placement::config()?;
    let (here, at, rules) = (crate::platform::host::name(), placement::now(), Rules::configured());
    let listings = fleets::everywhere(&cfg, &here, &["fleet", "list", "--json"], "fleet", at, PROBE, &mut Vec::new());
    let sessions: Vec<(Session, String)> = listings.iter().flat_map(|(host, listing)| fleets::running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    Ok((spend_view(&cfg, roster, &here, &rules, &sessions, at), rules))
}

/// NorthThreads claim listings from this host's ledger, every ssh host's, and the laptop's copy, since a host whose
/// ledger is local records its workers' claims there.
fn claim_listings(cfg: &DocumentMut, here: &str, at: f64, notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let remote = gate::machines(cfg).into_iter().filter(|machine| machine.name != here).map(|machine| (machine.name, Some(machine.ssh)));
    let mut found = Vec::new();
    for (host, ssh) in std::iter::once((here.to_owned(), None)).chain(remote) {
        match remote_json(ssh.as_deref(), &["threads", "list", "--json"]) {
            Ok(listing) => found.push((host, listing)),
            Err(why) => notes.push(format!("{host} threads list unread, so its claims are unknown: {why}")),
        }
    }
    found.extend(laptop_claims(cfg, here, &record::directory(), at, notes));
    found
}

/// The claim listing the laptop copies to `directory` as `threads-HOST.json` (south's north-dispatch-mirror), since no
/// ssh reaches its ledger and a laptop session's claim held nothing here before it was read.
pub(super) fn laptop_claims(cfg: &DocumentMut, here: &str, directory: &std::path::Path, at: f64, notes: &mut Vec<String>) -> Option<(String, Value)> {
    let laptop = fleets::laptop(cfg, here)?;
    let listing = fleets::mirrored(directory, "threads", &laptop, at, notes);
    Some((laptop, listing))
}

/// The holders in `threads list --json`: its live claims, the ones `threads claim` refuses on,
/// less those a stopped fleet session made.
pub fn claim_rows(listing: &Value, host: &str, stopped: &BTreeSet<String>) -> Vec<(IssueRef, String)> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["standing"] == "live")
        .filter(|row| row["box"].is_null())
        .filter(|row| !row["owner"].as_str().is_some_and(|owner| stopped.contains(owner)))
        .filter_map(|row| Some((IssueRef::parse(row["item"].as_str()?)?, format!("threads claim by {} on {host}", row["owner"].as_str()?))))
        .collect()
}

/// The boxes `threads list --json` shows live claims on, by item and box slug, less those a stopped fleet session made.
pub fn claim_box_rows(listing: &Value, host: &str, stopped: &BTreeSet<String>) -> Vec<(IssueRef, String, String)> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["standing"] == "live")
        .filter(|row| !row["owner"].as_str().is_some_and(|owner| stopped.contains(owner)))
        .filter_map(|row| Some((IssueRef::parse(row["item"].as_str()?)?, row["box"].as_str()?.to_owned(), format!("threads claim by {} on {host}", row["owner"].as_str()?))))
        .collect()
}

fn cloud(cfg: &DocumentMut, here: &str, at: f64) -> Cloud {
    let (usable, line) = placement::cloud_credit(cfg, at);
    if !usable {
        return Cloud::Closed(line);
    }
    let host = cfg.get("placement").and_then(|rules| rules.get("cloud_host")).and_then(Item::as_str);
    if host != Some(here) {
        return Cloud::Unreachable(host.map_or_else(|| "no [placement] cloud_host".into(), |host| format!("the credit login is on {host}")));
    }
    let north = std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "north".into());
    match run_within(&north, &["cloud", "list"], PROBE) {
        Some(output) if output.status.success() => Cloud::Reachable(line),
        Some(output) => Cloud::Unreachable(format!("north cloud list: {}", String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("failed"))),
        None => Cloud::Unreachable("north cloud list timed out".into()),
    }
}

/// The laptop's working sessions: those its copied fleet list shows, or dispatch's recent launches
/// there when they are more, since a fresh launch may not show yet.
pub fn laptop_running(sessions: &[(Session, String)], laptop: &str, dispatched: i64) -> i64 {
    let listed = sessions.iter().filter(|(session, host)| session.working && host == laptop).count();
    i64::try_from(listed).unwrap_or(i64::MAX).max(dispatched)
}

/// A dispatched session's `north fleet show --json`: a launch registers where dispatch ran, except on the laptop,
/// whose job queue starts it in the laptop's own registry and whose retired sessions it mirrors as `reports`.
pub(super) fn shown(launch: &record::Record, laptop: Option<&str>, reports: &Value) -> Result<Value, String> {
    if laptop == Some(launch.host.as_str()) {
        return reports.get(&launch.name).cloned().ok_or_else(|| format!("{} runs in the laptop's registry and its report is not among those mirrored", launch.name));
    }
    remote_json(None, &["fleet", "show", &launch.name, "--json"])
}

/// Reads the holds of each open box's last dispatched session's final report, from the session while the fleet lists it
/// and from its archive once the reaper has retired it.
pub(super) fn stopped(records: &[record::Record], issues: &mut [Issue], show: impl Fn(&record::Record) -> Result<Value, String>, retired: impl Fn(&str) -> Option<RetiredReport>, grounds: &Ledger, notes: &mut Vec<String>) {
    let launches = super::stops::last_launches(records);
    for issue in issues.iter_mut().filter(|issue| issue.unticked() > 0) {
        let Some(launch) = launches.get(&issue.reference()) else { continue };
        let evidence = super::spin::Evidence { reopened: issue.reopened(launch.box_text.as_deref()), time: grounds.time(&launch.name) };
        match show(launch) {
            Ok(session) => issue.stop = super::stops::stop(&session, launch, &evidence),
            Err(why) => match retired(&launch.name) {
                Some(report) => issue.stop = super::stops::retired_stop(&report, launch, &evidence),
                None => notes.push(format!("{} report unread, so {} may relaunch: {why}", launch.name, issue.reference())),
            },
        }
    }
}

/// The sessions of contested tries that did no work on their item, each with why its try is not spent.
/// A forgotten session's report went with it, so its try is not counted against the item.
pub(super) fn unspent(records: &[record::Record], at: f64, forgotten: &BTreeSet<String>, report: impl Fn(&record::Record) -> Result<Value, String>, issues: &[Issue], grounds: &Ledger, notes: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for launch in record::contested(records, at) {
        let issue = IssueRef::parse(&launch.item).and_then(|item| issues.iter().find(|issue| issue.reference() == item));
        let evidence = super::spin::Evidence { reopened: issue.is_some_and(|issue| issue.reopened(launch.box_text.as_deref())), time: grounds.time(&launch.name) };
        match report(launch) {
            Ok(session) => found.extend(super::tries::unspent(&session, &evidence).map(|why| (launch.name.clone(), why))),
            Err(_) if forgotten.contains(&launch.name) => {
                found.insert(launch.name.clone(), "was forgotten before its report was read".to_owned());
            }
            Err(why) => notes.push(format!("{} report unread, so its try counts: {why}", launch.name)),
        }
    }
    found
}

const DEGRADED: &str = "tier routing degraded";

fn closed(notes: &mut Vec<String>) -> Option<ClosedReport> {
    read_closed(crate::ledger::closed_report(tier::ARM_WINDOW), notes)
}

pub(super) fn read_closed(report: NorthResult<ClosedReport>, notes: &mut Vec<String>) -> Option<ClosedReport> {
    report.map_err(|why| notes.push(format!("{DEGRADED}: north dash --closed --since {} unread, so the arms draw at equal shares: {}", tier::ARM_WINDOW, why.user_message()))).ok()
}

pub fn evidence(categories: &[&str], notes: &mut Vec<String>) -> BTreeMap<String, Evidence> {
    let closed = closed(notes);
    evidence_from(categories, closed.as_ref())
}

fn evidence_from(categories: &[&str], closed: Option<&ClosedReport>) -> BTreeMap<String, Evidence> {
    let Some(report) = closed else { return BTreeMap::new() };
    categories.iter().map(|category| ((*category).to_owned(), Evidence::default().priced(report, category))).collect()
}

fn read<'scope, T: Send + 'scope>(scope: &'scope std::thread::Scope<'scope, '_>, reader: impl FnOnce(&mut Vec<String>) -> T + Send + 'scope) -> std::thread::ScopedJoinHandle<'scope, (T, Vec<String>)> {
    scope.spawn(move || {
        let mut notes = Vec::new();
        (reader(&mut notes), notes)
    })
}

fn joined<T>(handle: std::thread::ScopedJoinHandle<'_, (T, Vec<String>)>, notes: &mut Vec<String>) -> T {
    let (value, read) = handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    notes.extend(read);
    value
}

pub fn survey(roster: &Roster) -> Result<Survey, String> {
    let cfg = placement::config()?;
    let projects = Projects::read(&cfg)?;
    let here = crate::platform::host::name();
    let at = placement::now();
    let mut notes = Vec::new();
    let laptop = fleets::laptop(&cfg, &here);
    let (cfg_ref, here_ref, repos) = (&cfg, here.as_str(), projects.repos());
    let (fleet_lists, gates, (mut issues, open), claim_lists, (report, ledger), remote_arms, grounds, cloud, strands) = std::thread::scope(|scope| {
        let fleet_lists = read(scope, |notes| fleets::everywhere(cfg_ref, here_ref, &["fleet", "list", "--json"], "fleet", at, PROBE, notes));
        let gates = read(scope, |notes| fleets::everywhere(cfg_ref, here_ref, &["gate", "list", "--json"], "gates", at, PROBE, notes));
        let issues = read(scope, |notes| github::open_issues(repos, at, notes));
        let claim_lists = read(scope, |notes| claim_listings(cfg_ref, here_ref, at, notes));
        let evidence = read(scope, |notes| {
            let report = closed(notes);
            let ledger = evidence_from(&tier::evidenced(), report.as_ref());
            (report, ledger)
        });
        let remote_arms = read(scope, |notes| remote_arm_boxes(cfg_ref, here_ref, notes));
        let grounds = read(scope, Ledger::read);
        let cloud = read(scope, |_| cloud(cfg_ref, here_ref, at));
        let strands = read(scope, |notes| super::strands::read(cfg_ref, here_ref, at, notes));
        (
            joined(fleet_lists, &mut notes),
            joined(gates, &mut notes),
            joined(issues, &mut notes),
            joined(claim_lists, &mut notes),
            joined(evidence, &mut notes),
            joined(remote_arms, &mut notes),
            joined(grounds, &mut notes),
            joined(cloud, &mut notes),
            joined(strands, &mut notes),
        )
    });
    let sessions: Vec<(Session, String)> = fleet_lists.iter().flat_map(|(host, listing)| fleets::running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    let (elsewhere, mut holds) = fleets::tally(&sessions);
    let live: BTreeSet<String> = sessions.iter().map(|(session, _)| session.name.clone()).collect();
    let records = record::read(&record::directory());
    let stopped_sessions = fleets::stopped_names(&fleet_lists);
    holds.extend(claim_lists.iter().flat_map(|(host, listing)| claim_rows(listing, host, &stopped_sessions)));
    let mut box_holds = fleets::box_tally(&sessions);
    for (item, slug, why) in claim_lists.iter().flat_map(|(host, listing)| claim_box_rows(listing, host, &stopped_sessions)) {
        box_holds.entry(item).or_default().entry(slug).or_insert(why);
    }
    let laptop_reports = laptop.as_deref().map_or(Value::Null, |host| fleets::mirrored_reports(&record::directory(), host, &mut notes));
    let copies = fleets::copy_times(&record::directory(), "fleet", laptop.as_deref());
    let forgotten = fleets::forgotten(&fleet_lists, &copies, &records, at);
    let listed = fleets::listed(&fleet_lists);
    let unspent = unspent(&records, at, &forgotten, |launch| shown(launch, laptop.as_deref(), &laptop_reports), &issues, &grounds, &mut notes);
    holds.extend(record::holds(&records, at, &unspent, &fleets::gone(&fleet_lists, &copies, &records, at)));
    let mut box_tries = record::BoxHolds::new();
    for (item, boxes) in record::box_retries(&records, at, &unspent) {
        let Some(issue) = issues.iter().find(|issue| issue.reference() == item) else { continue };
        let unticked: BTreeSet<String> = issue.takeable(at).filter_map(crate::threads::claims::box_slug).collect();
        for (slug, why) in boxes.into_iter().filter(|(slug, _)| unticked.contains(slug)) {
            box_tries.entry(item.clone()).or_default().insert(slug, why);
        }
    }
    for (item, boxes) in record::box_launching(&records, at, &fleets::gone(&fleet_lists, &copies, &records, at)) {
        for (slug, why) in boxes {
            box_holds.entry(item.clone()).or_default().entry(slug).or_insert(why);
        }
    }
    let mut held: Vec<(IssueRef, String)> = gates.iter().flat_map(|(host, listing)| gate_rows(listing, host)).collect();
    held.sort();
    held.dedup();
    holds.extend(held);
    let on_laptop = laptop.as_deref().map_or(0, |name| laptop_running(&sessions, name, record::recent_on(&records, name, &open, at)));
    let names: Vec<String> = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter().map(|(name, _)| name.to_owned())).collect();
    let hosts = names.iter().map(|name| host(&cfg, name, &here, elsewhere.get(name).copied().unwrap_or(0), on_laptop)).collect();
    let accounts = accounts(&cfg, roster, &here, at, &mut notes);
    if accounts.is_empty() {
        notes.push("usage-gate.json names no Claude account, so no slot is free".into());
    }
    let rules = Rules::configured();
    let spend = spend_view(&cfg, roster, &here, &rules, &sessions, at);
    stopped(&records, &mut issues, |launch| shown(launch, laptop.as_deref(), &laptop_reports), crate::fleet::registry::retired_report_here, &grounds, &mut notes);
    let launches = super::stops::last_launches(&records);
    let learned = super::host_needs::read(&record::directory());
    let (waits, fresh_waits) = super::wait_stops::learn(&record::directory(), &issues, |issue| launches.get(&issue.reference()).map(|launch| launch.name.clone()), at).unwrap_or_else(|why| {
        notes.push(format!("wait-stops.jsonl unwritten, so this pass reads only the waits already in it: {why}"));
        (super::wait_stops::read(&record::directory()), Vec::new())
    });
    for issue in &mut issues {
        issue.needs = super::host_needs::of(issue, &learned, at);
        let reference = issue.reference();
        issue.stop.relay = launches.get(&reference).and_then(|launch| super::host_needs::relay(&learned, &reference.to_string(), &launch.name));
        issue.learned_wait = super::wait_stops::holding(&waits, issue, at);
        if issue.windowed() {
            issue.landed = landings::landed_at(issue);
        }
        if issue.counts_landings() {
            issue.landings = landings::since(issue);
        }
    }
    let history = History::new(&records);
    let arm_boxes: Vec<ArmBox> = report.iter().flat_map(|report| report.arms.iter().cloned()).chain(remote_arms).collect();
    notes.extend(tier::context_figures(&arm_boxes));
    let cloud_running = record::recent_on(&records, super::cloud::HOST, &open, at);
    let super::strands::Read { strands, saturated, revived } = strands;
    let frozen = crate::platform::frozen_topics::load().map(crate::threads::catalog::canonical_topics).unwrap_or_else(|why| {
        notes.push(why);
        Vec::new()
    });
    let deepseek = super::deepseek::Seen::read(&here, laptop.as_deref(), &fleet_lists, &laptop_reports, &sessions, &records, &issues, at, &mut notes);
    let backlog = north_graph_ground::read_backlog().map_err(|why| notes.push(format!("the fact graph's backlog is unread, so dispatch orders by priority label and reads box waits only as issue-level text: {why}"))).ok();
    Ok(Survey { here, cfg, projects, hosts, accounts, spend, rules, issues, open, holds, box_holds, box_tries, live, listed, cloud, laptop, cloud_running, notes, frozen, history, ledger, grounds, strands, saturated, revived, backlog, fresh_waits, deepseek, at })
}

pub(super) mod github;
mod landings;

#[cfg(test)]
#[path = "survey_tests.rs"]
mod tests;

/// The arm boxes in every other host's closed-box report (`north dash --closed`).
fn remote_arm_boxes(cfg: &DocumentMut, here: &str, notes: &mut Vec<String>) -> Vec<ArmBox> {
    let remote = fleets::reached(cfg, here, &["dash", "--closed", "--since", tier::ARM_WINDOW], PROBE, notes);
    remote.into_iter().flat_map(|(_, listing)| serde_json::from_value::<Vec<ArmBox>>(listing["arms"].clone()).unwrap_or_default()).collect()
}

