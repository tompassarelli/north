//! The live inputs one dispatch run reads: South's orchestration, pace-control's samples and
//! placement's quota verdicts, each host's resident sessions, fleet roster and open gates,
//! NorthThreads claims, the dispatch record and the open issues of the staffed repositories.
use super::issues::{self, Issue, IssueRef, OWNER};
use super::plan::Cloud;
use super::record;
use super::slots::{Account, Host, PlanSize, Room};
use super::projects::Projects;
use super::tier::{self, Evidence, History};
use super::handoff::Arms;
use super::grounds::Ledger;
use super::fleets::{self, PROBE, Session, remote_json};
use crate::fleet::model::ProviderName;
use crate::fleet::roster::{Roster, Standing};
use crate::pace::exhaustion;
use crate::ops::pacing::placement::{self, gate};
use crate::platform::process::run_within;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use toml_edit::{DocumentMut, Item};

pub(super) const CATALOG: &str = include_str!("../../../harness/model-catalog.toml");
const PACE_FRESH_S: f64 = 15.0 * 60.0;
const WEEK_H: f64 = 7.0 * 24.0;

pub(crate) struct Survey {
    pub here: String,
    pub cfg: DocumentMut,
    pub projects: Projects,
    pub hosts: Vec<Host>,
    pub accounts: Vec<Account>,
    pub issues: Vec<Issue>,
    pub open: BTreeSet<IssueRef>,
    pub holds: BTreeMap<IssueRef, String>,
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
    /// `worker-ledger --recommend` for each armed category.
    pub ledger: BTreeMap<String, Evidence>,
    /// The closed boxes and prior reports each brief grounds its ETA, budget and trace in.
    pub grounds: Ledger,
    pub at: f64,
}


fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn host_table<'a>(cfg: &'a DocumentMut, name: &str) -> Option<&'a Item> {
    cfg.get("hosts")?.get(name)
}

/// The issues the open gates in one host's `north gate list --json` name, in the question or by
/// the asking session's name, each with the gate that holds it.
pub(crate) fn gate_rows(listing: &Value, host: &str) -> Vec<(IssueRef, String)> {
    let mut found = Vec::new();
    for gate in listing.as_array().into_iter().flatten().filter(|gate| gate["closed_ms"].is_null()) {
        let session = gate["session"].as_str().unwrap_or_default();
        let repo = fleets::named_items(session).first().map_or_else(String::new, |item| item.repo.clone());
        let question = gate["question"].as_str().unwrap_or_default();
        let why = format!("open north gate {} on {host} from {session}, for Tom", gate["id"]);
        let mut items: Vec<IssueRef> = issues::references(question, &repo).into_iter().filter(|item| !item.repo.is_empty()).chain(fleets::named_items(session)).collect();
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
    Host { name: name.into(), cap, running, measure: super::measure::read(name == here, ssh) }
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
pub(crate) fn refusal(name: &str, control: &Value, verdict: Option<&gate::Verdict>, at: f64) -> Option<String> {
    let age = at - control["at"].as_f64().unwrap_or(0.0);
    if age > PACE_FRESH_S {
        return Some(format!("pace-control.json sampled {:.0} min ago", age / 60.0));
    }
    match verdict {
        None => Some(format!("{name} has no placement verdict, so dispatch staffs it nowhere")),
        Some(verdict) => (!verdict.available).then(|| verdict.line.clone()),
    }
}

/// The hosts an account runs on: those South signs it in on, less this host when this host's
/// roster has no login of its own for it.
pub(crate) fn signed_in(cfg: &DocumentMut, roster: &Roster, account: &str, here: &str) -> Vec<String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let own = matches!(roster.standing(&ProviderName::new(provider), label), Standing::Listed(_));
    account_hosts(cfg, account, here).into_iter().filter(|host| own || host != here).collect()
}

/// An account's plan size per window in Max 5x units: the five-hour and weekly multipliers of its
/// `[accounts.PROVIDER] plans` entry in the model catalog, over those of `max-5x`.
pub(crate) fn plan(cfg: &DocumentMut, catalog: &DocumentMut, account: &str) -> Result<PlanSize, String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let named = cfg.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|policy| policy.get("plans")).and_then(|plans| plans.get(label)).and_then(Item::as_str);
    let plan = named.ok_or_else(|| format!("{account} has no [accounts.{provider}] plans entry, so dispatch sizes it as max-5x"))?;
    let plans = catalog.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|catalog| catalog.get("plans"));
    let multiples = |name: &str| {
        let entry = plans?.get(name)?;
        let multiple = |key: &str| entry.get(key).and_then(|item| item.as_float().or_else(|| item.as_integer().map(|whole| whole as f64))).filter(|times| *times > 0.0);
        Some((multiple("five_hour_multiplier")?, multiple("weekly_multiplier")?))
    };
    let unit = multiples("max-5x").ok_or_else(|| format!("the model catalog has no {provider} max-5x plan with five_hour_multiplier and weekly_multiplier, so dispatch sizes {account} as max-5x"))?;
    let (five_hour, week) = multiples(plan).ok_or_else(|| format!("{account} plan {plan} has no five_hour_multiplier and weekly_multiplier in the model catalog, so dispatch sizes it as max-5x"))?;
    Ok(PlanSize { five_hour: five_hour / unit.0, week: week / unit.1 })
}

/// The room a usage-gate control leaves at `at`: its tightest five-hour and week windows, an
/// unsampled window counting as unused.
pub(crate) fn room(control: &Value, at: f64) -> Room {
    let mut room = Room { five_hour: exhaustion::FIVE_HOUR_STOP, week: 100.0, week_reset_h: WEEK_H };
    for window in control["windows"].as_array().into_iter().flatten() {
        let (Some(name), Some(used)) = (window["window"].as_str(), window["used_pct"].as_f64()) else { continue };
        let Some(left) = exhaustion::headroom(name, used) else { continue };
        if name == "5h" {
            room.five_hour = room.five_hour.min(left);
        } else if left <= room.week {
            let reset = window["resets_at"].as_str().and_then(crate::pace::target::seconds).map(|resets| (resets - at) / 3600.0);
            room = Room { week: left, week_reset_h: reset.or_else(|| window["reset_h"].as_f64()).unwrap_or(WEEK_H), ..room };
        }
    }
    room
}

/// The 5h window's measured burn in percent per hour, which usage-gate carries only for a trail of two readings.
fn five_hour_burn(usage: &Value) -> Option<f64> {
    usage["windows"].as_array()?.iter().find(|window| window["window"] == "5h")?["burn_pct_h"].as_f64()
}

fn accounts(cfg: &DocumentMut, roster: &Roster, here: &str, at: f64, notes: &mut Vec<String>) -> Vec<Account> {
    let pace = placement::pace_state("pace-control.json");
    let usage = placement::pace_state("usage-gate.json");
    let catalog: DocumentMut = CATALOG.parse().expect("harness/model-catalog.toml parses");
    pace["accounts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, control)| {
            let sampled = usage["claude"]["controls"].get(name).filter(|_| name.starts_with("claude:"));
            let verdict = sampled.map(|usage| gate::verdict(name, usage));
            let plan = match sampled.map(|_| plan(cfg, &catalog, name)) {
                Some(Ok(plan)) => plan,
                Some(Err(why)) => {
                    notes.push(why);
                    PlanSize { five_hour: 1.0, week: 1.0 }
                }
                None => PlanSize { five_hour: 1.0, week: 1.0 },
            };
            Account {
                name: name.clone(),
                hosts: signed_in(cfg, roster, name, here),
                plan,
                room: sampled.map_or(Room { five_hour: 0.0, week: 0.0, week_reset_h: WEEK_H }, |usage| room(usage, at)),
                spending: control["target"]["by"].as_str().and_then(crate::pace::target::seconds).is_some_and(|by| by > at),
                refusal: refusal(name, control, verdict.as_ref(), at),
                burn: sampled.and_then(five_hour_burn),
            }
        })
        .collect()
}

/// NorthThreads claims from this host's ledger and every ssh host's, since a host whose ledger is
/// local records its workers' claims there.
fn claims(cfg: &DocumentMut, here: &str, stopped: &BTreeSet<String>, notes: &mut Vec<String>) -> Vec<(IssueRef, String)> {
    let remote = gate::machines(cfg).into_iter().filter(|machine| machine.name != here).map(|machine| (machine.name, Some(machine.ssh)));
    let mut found = Vec::new();
    for (host, ssh) in std::iter::once((here.to_owned(), None)).chain(remote) {
        match remote_json(ssh.as_deref(), &["threads", "list", "--json"]) {
            Ok(listing) => found.extend(claim_rows(&listing, &host, stopped)),
            Err(why) => notes.push(format!("{host} threads list unread, so its claims are unknown: {why}")),
        }
    }
    found
}

/// The holders in `threads list --json`: its live claims, the ones `threads claim` refuses on,
/// less those a stopped fleet session made.
pub(crate) fn claim_rows(listing: &Value, host: &str, stopped: &BTreeSet<String>) -> Vec<(IssueRef, String)> {
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

/// The laptop's resident sessions: those its copied fleet list shows, or dispatch's recent launches
/// there when they are more, since a fresh launch may not show yet.
pub(crate) fn laptop_running(sessions: &[(Session, String)], laptop: &str, dispatched: i64) -> i64 {
    let listed = sessions.iter().filter(|(_, host)| host == laptop).count();
    i64::try_from(listed).unwrap_or(i64::MAX).max(dispatched)
}

/// A dispatched session's `north fleet show --json`: a launch registers where dispatch ran, except
/// on the laptop, whose job queue starts it in the laptop's own registry.
fn report(launch: &record::Record, laptop: Option<&str>) -> Result<Value, String> {
    if laptop == Some(launch.host.as_str()) {
        return Err(format!("{} runs in the laptop's registry", launch.name));
    }
    remote_json(None, &["fleet", "show", &launch.name, "--json"])
}

/// Marks each open box whose last dispatched session ended its report with `Stop: decision`.
fn stopped(laptop: Option<&str>, records: &[record::Record], issues: &mut [Issue], notes: &mut Vec<String>) {
    let launches = super::stops::last_launches(records);
    for issue in issues.iter_mut().filter(|issue| issue.unticked() > 0) {
        let Some(launch) = launches.get(&issue.reference()) else { continue };
        match report(launch, laptop) {
            Ok(session) => issue.stopped = super::stops::stopped_at(&session),
            Err(why) => notes.push(format!("{} report unread, so {} may relaunch: {why}", launch.name, issue.reference())),
        }
    }
}

/// The sessions of contested tries that did no work on their item, each with why its try is not spent.
fn unspent(laptop: Option<&str>, records: &[record::Record], at: f64, notes: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for launch in record::contested(records, at) {
        match report(launch, laptop) {
            Ok(session) => found.extend(super::tries::unspent(&session).map(|why| (launch.name.clone(), why))),
            Err(why) => notes.push(format!("{} report unread, so its try counts: {why}", launch.name)),
        }
    }
    found
}

pub(crate) fn evidence(categories: &[&str], notes: &mut Vec<String>) -> BTreeMap<String, Evidence> {
    let mut found = BTreeMap::new();
    let closed = crate::ledger::closed_report(tier::ARM_WINDOW).map_err(|why| notes.push(format!("north ledger summary --since {} unread, so the arms keep alternating: {}", tier::ARM_WINDOW, why.user_message())));
    for category in categories {
        let facets = format!("Category: {category}");
        let program = crate::fleet::runtime::binary("worker-ledger");
        match run_within(&program.to_string_lossy(), &["--recommend", &facets], PROBE) {
            Some(output) if output.status.success() => {
                let read = Evidence::parse(&String::from_utf8_lossy(&output.stdout), category);
                found.insert((*category).to_owned(), closed.as_ref().map_or(read.clone(), |report| read.priced(report, category)));
            }
            _ => notes.push(format!("worker-ledger --recommend {facets:?} unread, so {category} keeps alternating its arms")),
        }
    }
    found
}

pub(crate) fn survey(roster: &Roster) -> Result<Survey, String> {
    let cfg = placement::config()?;
    let projects = Projects::read(&cfg)?;
    let here = crate::platform::host::name();
    let at = placement::now();
    let mut notes = Vec::new();
    let laptop = fleets::laptop(&cfg, &here);
    let fleet_lists = fleets::everywhere(&cfg, &here, &["fleet", "list", "--json"], "fleet", at, &mut notes);
    let gates = fleets::everywhere(&cfg, &here, &["gate", "list", "--json"], "gates", at, &mut notes);
    let sessions: Vec<(Session, String)> = fleet_lists.iter().flat_map(|(host, listing)| fleets::running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    let (elsewhere, mut holds) = fleets::tally(&sessions);
    let (mut issues, open) = github::open_issues(projects.repos(), &mut notes);
    let records = record::read(&record::directory());
    holds.extend(claims(&cfg, &here, &fleets::stopped_names(&fleet_lists), &mut notes));
    holds.extend(record::holds(&records, at, &unspent(laptop.as_deref(), &records, at, &mut notes), &fleets::gone(&fleet_lists, &fleets::copy_times(&record::directory(), "fleet", laptop.as_deref()), &records, at)));
    holds.extend(gates.iter().flat_map(|(host, listing)| gate_rows(listing, host)));
    let on_laptop = laptop.as_deref().map_or(0, |name| laptop_running(&sessions, name, record::recent_on(&records, name, &open, at)));
    let names: Vec<String> = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter().map(|(name, _)| name.to_owned())).collect();
    let hosts = names.iter().map(|name| host(&cfg, name, &here, elsewhere.get(name).copied().unwrap_or(0), on_laptop)).collect();
    let accounts = accounts(&cfg, roster, &here, at, &mut notes);
    if accounts.is_empty() {
        notes.push("pace-control.json names no account, so no slot is free".into());
    }
    stopped(laptop.as_deref(), &records, &mut issues, &mut notes);
    let history = History::new(&records);
    let handoffs = Arms::new(&records);
    let ledger = evidence(&tier::ARMED, &mut notes);
    let grounds = Ledger::read(&mut notes);
    let cloud = cloud(&cfg, &here, at);
    let cloud_running = record::recent_on(&records, super::cloud::HOST, &open, at);
    let frozen = crate::platform::frozen_topics::load().unwrap_or_else(|why| {
        notes.push(why);
        Vec::new()
    });
    Ok(Survey { here, cfg, projects, hosts, accounts, issues, open, holds, cloud, laptop, cloud_running, notes, frozen, history, handoffs, ledger, grounds, at })
}

mod github {
    use super::*;

    const QUERY: &str = "query($owner: String!, $name: String!, $cursor: String) { repository(owner: $owner, name: $name) { issues(states: OPEN, first: 50, after: $cursor) { pageInfo { hasNextPage endCursor } nodes { number title body labels(first: 30) { nodes { name } } blockedBy(first: 30) { nodes { number state repository { name } } } lastEditedAt comments(last: 1) { nodes { body createdAt updatedAt } } } } } }";

    pub(crate) type OpenIssues = (Vec<Issue>, BTreeSet<IssueRef>);

    fn touched(node: &Value) -> f64 {
        let comment = &node["comments"]["nodes"][0];
        [&node["lastEditedAt"], &comment["createdAt"], &comment["updatedAt"]].into_iter().filter_map(|time| crate::pace::target::seconds(time.as_str()?)).fold(0.0, f64::max)
    }

    pub(crate) fn issue(repo: &str, node: &Value) -> Option<Issue> {
        let open_blockers = node["blockedBy"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|blocker| blocker["state"] == "OPEN")
            .filter_map(|blocker| Some(IssueRef::new(blocker["repository"]["name"].as_str()?, blocker["number"].as_u64()?)))
            .collect();
        Some(Issue {
            repo: repo.into(),
            number: node["number"].as_u64()?,
            title: node["title"].as_str().unwrap_or_default().into(),
            body: node["body"].as_str().unwrap_or_default().into(),
            labels: node["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str().map(str::to_owned)).collect(),
            open_blockers,
            report: node["comments"]["nodes"][0]["body"].as_str().unwrap_or_default().into(),
            touched: touched(node),
            stopped: None,
        })
    }

    fn page(repo: &str, cursor: Option<&str>) -> Result<Value, String> {
        let (query, owner, name) = (format!("query={QUERY}"), format!("owner={OWNER}"), format!("name={repo}"));
        let mut args = vec!["api", "graphql", "-f", &query, "-F", &owner, "-F", &name];
        let after = cursor.map(|cursor| format!("cursor={cursor}"));
        if let Some(after) = &after {
            args.extend(["-F", after]);
        }
        crate::platform::github::json(&args).map(|value| value["data"]["repository"]["issues"].clone())
    }

    fn repository(repo: &str) -> Result<Vec<Issue>, String> {
        let (mut found, mut cursor) = (Vec::new(), None::<String>);
        loop {
            let issues = page(repo, cursor.as_deref())?;
            found.extend(issues["nodes"].as_array().into_iter().flatten().filter_map(|node| issue(repo, node)));
            if issues["pageInfo"]["hasNextPage"] != true {
                return Ok(found);
            }
            cursor = issues["pageInfo"]["endCursor"].as_str().map(str::to_owned);
        }
    }

    fn still_open(reference: &IssueRef) -> Option<bool> {
        let (number, repo) = (reference.number.to_string(), format!("{OWNER}/{}", reference.repo));
        let state = crate::platform::github::json(&["issue", "view", &number, "-R", &repo, "--json", "state"]).ok()?;
        Some(state["state"] == "OPEN")
    }

    /// The staffed repositories' open issues, and every open issue they or their plans wait on.
    pub(crate) fn open_issues(repos: &[String], notes: &mut Vec<String>) -> OpenIssues {
        let mut found = Vec::new();
        for repo in repos {
            match repository(repo) {
                Ok(issues) => found.extend(issues),
                Err(why) => notes.push(format!("{repo} issues unread: {why}")),
            }
        }
        let mut open: BTreeSet<IssueRef> = found.iter().map(Issue::reference).collect();
        let planned = issues::planned_predecessors(&found);
        let named = found.iter().flat_map(|issue| issues::predecessors(issue).unwrap_or_default()).chain(planned.into_values().flatten());
        let elsewhere: BTreeSet<IssueRef> = named.filter(|reference| !repos.contains(&reference.repo)).collect();
        for reference in elsewhere {
            if still_open(&reference) != Some(false) {
                open.insert(reference);
            }
        }
        (found, open)
    }
}
