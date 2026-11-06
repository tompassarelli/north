//! The live inputs one dispatch run reads: South's orchestration, pace-control's samples and
//! placement's quota verdicts, each host's resident sessions, fleet roster and open gates,
//! NorthThreads claims, the dispatch record and the open issues of the staffed repositories.
use super::issues::{self, Issue, IssueRef, OWNER};
use super::plan::Cloud;
use super::record;
use super::slots::{Account, Host, PlanSize, Room};
use super::projects::Projects;
use super::tier::{self, Evidence, History};
use super::handoff::{self, Arms};
use super::grounds::Ledger;
use super::fleets::{self, PROBE, Session, remote_json};
use super::revive::Strand;
use crate::error::NorthResult;
use crate::fleet::model::ProviderName;
use crate::ledger::ClosedReport;
use crate::fleet::roster::{Roster, Standing};
use crate::pacing::placement::{self, gate};
use crate::pace::projection;
use crate::platform::process::run_within;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item};

pub(super) use crate::pacing::placement::room::room;
use crate::pacing::placement::room::{Basis, WEEK_H};
const PACE_FRESH_S: f64 = 15.0 * 60.0;

pub struct Survey {
    pub here: String,
    pub cfg: DocumentMut,
    pub projects: Projects,
    pub hosts: Vec<Host>,
    pub accounts: Vec<Account>,
    pub issues: Vec<Issue>,
    pub open: BTreeSet<IssueRef>,
    pub holds: BTreeMap<IssueRef, String>,
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
    pub handoffs: Arms,
    /// `north worker-ledger --recommend` for each armed category.
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

/// Why dispatch gives an account no slot: a stale pace sample, the out-rule as placement's verdict
/// applies it (`crate::pace::exhaustion`), or no usage sample to judge it by.
pub fn refusal(name: &str, control: &Value, verdict: Option<&gate::Verdict>, at: f64) -> Option<String> {
    let age = at - control["at"].as_f64().unwrap_or(0.0);
    if age > PACE_FRESH_S {
        return Some(format!("pace-control.json sampled {:.0} min ago", age / 60.0));
    }
    match verdict {
        None => Some(format!("{name} has no placement verdict, so dispatch staffs it nowhere")),
        Some(verdict) => (!verdict.available).then(|| verdict.line.clone()),
    }
}

/// `refusal`, else the week-room rule `Room::week_short` that keeps a box off an account the
/// provider has not yet exhausted.
pub fn account_refusal(name: &str, control: &Value, verdict: Option<&gate::Verdict>, room: &Room, at: f64) -> Option<String> {
    refusal(name, control, verdict, at).or_else(|| room.week_short(Room::box_reserve()).map(|why| format!("{name} unavailable: {why}")))
}

/// The hosts an account runs on: those South signs it in on, less this host when this host's
/// roster has no login of its own for it.
pub fn signed_in(cfg: &DocumentMut, roster: &Roster, account: &str, here: &str) -> Vec<String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let own = matches!(roster.standing(&ProviderName::new(provider), label), Standing::Listed(_));
    account_hosts(cfg, account, here).into_iter().filter(|host| own || host != here).collect()
}

/// The 5h window's measured burn in percent per hour, which usage-gate carries only for a trail of
/// two readings; a window whose reset has passed is a stale sample whose flat trail measures nothing.
fn five_hour_burn(usage: &Value, at: f64) -> Option<f64> {
    let window = usage["windows"].as_array()?.iter().find(|window| window["window"] == "5h")?;
    let resets = window["resets_at"].as_str().and_then(crate::pace::target::seconds);
    window["burn_pct_h"].as_f64().filter(|_| resets.is_none_or(|resets| resets > at))
}

/// The sessions pace-control leaves an account, held or not: its `allowed_workers` less the
/// `working` sessions on it; None when pace names no `allowed_workers`.
pub fn pace_workers(control: &Value, working: i64) -> Option<i64> {
    control["allowed_workers"].as_i64().map(|allowed| (allowed - working).max(0))
}

/// What dispatch knows of one account at `at`: its pace-control entry, its usage-gate control when
/// it is a sampled Claude account, and the sessions working on it.
pub struct Sample<'a> {
    pub name: &'a str,
    pub control: &'a Value,
    pub usage: Option<&'a Value>,
    pub working: i64,
}

pub fn account(sample: &Sample, basis: &Basis, hosts: Vec<String>, at: f64, notes: &mut Vec<String>) -> Account {
    let Sample { name, control, usage: sampled, working } = *sample;
    let verdict = sampled.map(|usage| gate::verdict(name, usage));
    let plan = match sampled.map(|_| basis.plan(name)) {
        Some(Ok(plan)) => plan,
        Some(Err(why)) => {
            notes.push(why);
            PlanSize { five_hour: 1.0, week: 1.0 }
        }
        None => PlanSize { five_hour: 1.0, week: 1.0 },
    };
    let room = sampled.map_or(Room { five_hour: 0.0, week: 0.0, week_reset_h: WEEK_H }, |usage| room(usage, at));
    Account {
        name: name.to_owned(),
        hosts,
        plan,
        spending: basis.spending(name),
        refusal: account_refusal(name, control, verdict.as_ref(), &room, at),
        room,
        burn: sampled.and_then(|usage| five_hour_burn(usage, at)),
        readings: sampled.map(projection::readings).unwrap_or_default(),
        workers: pace_workers(control, working),
    }
}

fn accounts(cfg: &DocumentMut, roster: &Roster, here: &str, at: f64, working: &BTreeMap<String, i64>, notes: &mut Vec<String>) -> Vec<Account> {
    let pace = placement::pace_state("pace-control.json");
    let usage = placement::pace_state("usage-gate.json");
    let basis = Basis::new(cfg, pace.clone(), at);
    pace["accounts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, control)| {
            let sample = Sample { name, control, usage: usage["claude"]["controls"].get(name).filter(|_| name.starts_with("claude:")), working: working.get(name).copied().unwrap_or(0) };
            account(&sample, &basis, signed_in(cfg, roster, name, here), at, notes)
        })
        .collect()
}

/// NorthThreads claim listings from this host's ledger and every ssh host's, since a host whose
/// ledger is local records its workers' claims there.
fn claim_listings(cfg: &DocumentMut, here: &str, notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let remote = gate::machines(cfg).into_iter().filter(|machine| machine.name != here).map(|machine| (machine.name, Some(machine.ssh)));
    let mut found = Vec::new();
    for (host, ssh) in std::iter::once((here.to_owned(), None)).chain(remote) {
        match remote_json(ssh.as_deref(), &["threads", "list", "--json"]) {
            Ok(listing) => found.push((host, listing)),
            Err(why) => notes.push(format!("{host} threads list unread, so its claims are unknown: {why}")),
        }
    }
    found
}

/// The holders in `threads list --json`: its live claims, the ones `threads claim` refuses on,
/// less those a stopped fleet session made.
pub fn claim_rows(listing: &Value, host: &str, stopped: &BTreeSet<String>) -> Vec<(IssueRef, String)> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["standing"] == "live")
        .filter(|row| !row["owner"].as_str().is_some_and(|owner| stopped.contains(owner)))
        .filter_map(|row| Some((IssueRef::parse(row["item"].as_str()?)?, format!("threads claim by {} on {host}", row["owner"].as_str()?))))
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

/// A dispatched session's `north fleet show --json`: a launch registers where dispatch ran, except
/// on the laptop, whose job queue starts it in the laptop's own registry.
fn shown(launch: &record::Record, laptop: Option<&str>) -> Result<Value, String> {
    if laptop == Some(launch.host.as_str()) {
        return Err(format!("{} runs in the laptop's registry", launch.name));
    }
    remote_json(None, &["fleet", "show", &launch.name, "--json"])
}

/// Reads the holds of each open box's last dispatched session's final report.
fn stopped(laptop: Option<&str>, records: &[record::Record], issues: &mut [Issue], notes: &mut Vec<String>) {
    let launches = super::stops::last_launches(records);
    for issue in issues.iter_mut().filter(|issue| issue.unticked() > 0) {
        let Some(launch) = launches.get(&issue.reference()) else { continue };
        match shown(launch, laptop) {
            Ok(session) => issue.stop = super::stops::stop(&session),
            Err(why) => notes.push(format!("{} report unread, so {} may relaunch: {why}", launch.name, issue.reference())),
        }
    }
}

/// The sessions of contested tries that did no work on their item, each with why its try is not spent.
/// A forgotten session's report went with it, so its try is not counted against the item.
pub(super) fn unspent(records: &[record::Record], at: f64, forgotten: &BTreeSet<String>, report: impl Fn(&record::Record) -> Result<Value, String>, notes: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for launch in record::contested(records, at) {
        match report(launch) {
            Ok(session) => found.extend(super::tries::unspent(&session).map(|why| (launch.name.clone(), why))),
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
    report.map_err(|why| notes.push(format!("{DEGRADED}: north ledger summary --since {} unread, so the arms keep alternating: {}", tier::ARM_WINDOW, why.user_message()))).ok()
}

pub(super) fn read_arm_runs(category: &str, runs: NorthResult<BTreeMap<String, usize>>, notes: &mut Vec<String>) -> BTreeMap<String, usize> {
    runs.map_err(|why| notes.push(format!("{DEGRADED}: arm runs of {category} unread, so its arms keep alternating: {}", why.user_message()))).unwrap_or_default()
}

pub fn evidence(categories: &[&str], notes: &mut Vec<String>) -> BTreeMap<String, Evidence> {
    let closed = closed(notes);
    evidence_from(categories, closed.as_ref(), notes)
}

fn evidence_from(categories: &[&str], closed: Option<&ClosedReport>, notes: &mut Vec<String>) -> BTreeMap<String, Evidence> {
    let mut found = BTreeMap::new();
    let north = crate::fleet::runtime::binary("north");
    let routes = super::tuned::current();
    let efforts = super::tuned::current_efforts();
    for category in categories {
        let facets = format!("Category: {category}");
        match run_within(&north.to_string_lossy(), &["worker-ledger", "--recommend", &facets], PROBE) {
            Some(output) if output.status.success() => {
                let read = Evidence::parse(&String::from_utf8_lossy(&output.stdout), category);
                let read = read.counted(&read_arm_runs(category, crate::ledger::arm_runs(category), notes));
                found.insert((*category).to_owned(), closed.map_or(read.clone(), |report| read.priced(report, category)).tuned(&routes, category).tuned_effort(&efforts, category));
            }
            _ => {
                notes.push(format!("{DEGRADED}: north worker-ledger --recommend {facets:?} unread, so {category} keeps alternating its arms unless north tune routed it"));
                if routes.contains_key(*category) || efforts.contains_key(*category) {
                    found.insert((*category).to_owned(), Evidence::default().tuned(&routes, category).tuned_effort(&efforts, category));
                }
            }
        }
    }
    found
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
        let claim_lists = read(scope, |notes| claim_listings(cfg_ref, here_ref, notes));
        let evidence = read(scope, |notes| {
            let report = closed(notes);
            let ledger = evidence_from(&tier::evidenced(), report.as_ref(), notes);
            (report, ledger)
        });
        let remote_arms = read(scope, |notes| handoff::remote(cfg_ref, here_ref, notes));
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
    let stopped_sessions = fleets::exited_names(&fleet_lists);
    holds.extend(claim_lists.iter().flat_map(|(host, listing)| claim_rows(listing, host, &stopped_sessions)));
    let copies = fleets::copy_times(&record::directory(), "fleet", laptop.as_deref());
    let forgotten = fleets::forgotten(&fleet_lists, &copies, &records, at);
    let listed = fleets::listed(&fleet_lists);
    let unspent = unspent(&records, at, &forgotten, |launch| shown(launch, laptop.as_deref()), &mut notes);
    holds.extend(record::holds(&records, at, &unspent, &fleets::gone(&fleet_lists, &copies, &records, at)));
    let mut held: Vec<(IssueRef, String)> = gates.iter().flat_map(|(host, listing)| gate_rows(listing, host)).collect();
    held.sort();
    held.dedup();
    holds.extend(held);
    let on_laptop = laptop.as_deref().map_or(0, |name| laptop_running(&sessions, name, record::recent_on(&records, name, &open, at)));
    let names: Vec<String> = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter().map(|(name, _)| name.to_owned())).collect();
    let hosts = names.iter().map(|name| host(&cfg, name, &here, elsewhere.get(name).copied().unwrap_or(0), on_laptop)).collect();
    let accounts = accounts(&cfg, roster, &here, at, &fleets::working_by_account(&sessions), &mut notes);
    if accounts.is_empty() {
        notes.push("pace-control.json names no account, so no slot is free".into());
    }
    stopped(laptop.as_deref(), &records, &mut issues, &mut notes);
    let history = History::new(&records);
    let handoffs = Arms::new(&records).priced(&handoff::boxes(report.as_ref(), remote_arms));
    notes.extend(handoffs.recommendations());
    let cloud_running = record::recent_on(&records, super::cloud::HOST, &open, at);
    let super::strands::Read { strands, saturated, revived } = strands;
    let frozen = crate::platform::frozen_topics::load().map(crate::threads::catalog::canonical_topics).unwrap_or_else(|why| {
        notes.push(why);
        Vec::new()
    });
    let backlog = north_graph_ground::read_backlog().map_err(|why| notes.push(format!("the fact graph's backlog is unread, so dispatch orders by priority label and reads box waits only as issue-level text: {why}"))).ok();
    Ok(Survey { here, cfg, projects, hosts, accounts, issues, open, holds, live, listed, cloud, laptop, cloud_running, notes, frozen, history, handoffs, ledger, grounds, strands, saturated, revived, backlog, at })
}

mod github {
    use super::*;
    use north_github_mirror::store::{self, Mirror, OPEN_ISSUES, key};

    pub type OpenIssues = (Vec<Issue>, BTreeSet<IssueRef>);

    fn touched(node: &Value) -> f64 {
        let comment = node["comments"]["nodes"].as_array().and_then(|nodes| nodes.last()).unwrap_or(&Value::Null);
        [&node["lastEditedAt"], &comment["createdAt"], &comment["updatedAt"]].into_iter().filter_map(|time| crate::pace::target::seconds(time.as_str()?)).fold(0.0, f64::max)
    }

    pub fn issue(repo: &str, node: &Value) -> Option<Issue> {
        let open_blockers = node["blockedBy"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|blocker| blocker["state"] == "OPEN")
            .filter_map(|blocker| Some(IssueRef::new(blocker["repository"]["name"].as_str()?, blocker["number"].as_u64()?)))
            .collect();
        let comments: Vec<Value> = node["comments"]["nodes"].as_array().cloned().unwrap_or_default();
        Some(Issue {
            repo: repo.into(),
            number: node["number"].as_u64()?,
            title: node["title"].as_str().unwrap_or_default().into(),
            body: node["body"].as_str().unwrap_or_default().into(),
            labels: node["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str().map(str::to_owned)).collect(),
            open_blockers,
            report: comments.last().and_then(|comment| comment["body"].as_str()).unwrap_or_default().into(),
            comments,
            touched: touched(node),
            created: node["createdAt"].as_str().and_then(crate::pace::target::seconds).unwrap_or(0.0),
            stop: Default::default(),
        })
    }

    pub trait Upstream {
        fn open_issue_nodes(&self, repo: &str) -> Result<Vec<Value>, String>;
        fn still_open(&self, reference: &IssueRef) -> Option<bool>;
    }

    pub struct Live;

    impl Upstream for Live {
        fn open_issue_nodes(&self, repo: &str) -> Result<Vec<Value>, String> {
            store::fetch_open_issues(repo)
        }

        fn still_open(&self, reference: &IssueRef) -> Option<bool> {
            let (number, repo) = (reference.number.to_string(), format!("{OWNER}/{}", reference.repo));
            let state = crate::platform::github::json(&["issue", "view", &number, "-R", &repo, "--json", "state"]).ok()?;
            Some(state["state"] == "OPEN")
        }
    }

    fn nodes(repo: &str, mirror: &Mirror, at: f64, upstream: &dyn Upstream, notes: &mut Vec<String>) -> Result<Vec<Value>, String> {
        if let Some(Value::Array(nodes)) = mirror.fresh(&key(repo, OPEN_ISSUES), at) {
            return Ok(nodes);
        }
        notes.push(format!("{repo} issues read from GitHub: {} holds no copy under {:.0} min old", mirror.path().display(), store::FRESH_S / 60.0));
        upstream.open_issue_nodes(repo)
    }

    fn mirrored_open(reference: &IssueRef, mirror: &Mirror, at: f64) -> Option<bool> {
        let nodes = mirror.fresh(&key(&reference.repo, OPEN_ISSUES), at)?;
        Some(nodes.as_array()?.iter().any(|node| node["number"].as_u64() == Some(reference.number)))
    }

    pub fn open_issues(repos: &[String], at: f64, notes: &mut Vec<String>) -> OpenIssues {
        read_open_issues(repos, &Mirror::local(), at, &Live, notes)
    }

    /// The staffed repositories' open issues, and every open issue they or their plans wait on, read from the GitHub mirror while it holds a fresh copy.
    pub fn read_open_issues(repos: &[String], mirror: &Mirror, at: f64, upstream: &dyn Upstream, notes: &mut Vec<String>) -> OpenIssues {
        let mut found = Vec::new();
        for repo in repos {
            match nodes(repo, mirror, at, upstream, notes) {
                Ok(nodes) => found.extend(nodes.iter().filter_map(|node| issue(repo, node))),
                Err(why) => notes.push(format!("{repo} issues unread: {why}")),
            }
        }
        let mut open: BTreeSet<IssueRef> = found.iter().map(Issue::reference).collect();
        let planned = issues::planned_predecessors(&found);
        let named = found.iter().flat_map(|issue| issues::predecessors(issue).unwrap_or_default()).chain(planned.into_values().flatten());
        let elsewhere: BTreeSet<IssueRef> = named.filter(|reference| !repos.contains(&reference.repo)).collect();
        for reference in elsewhere {
            if mirrored_open(&reference, mirror, at).or_else(|| upstream.still_open(&reference)) != Some(false) {
                open.insert(reference);
            }
        }
        (found, open)
    }
}

#[cfg(test)]
#[path = "survey_tests.rs"]
mod tests;
