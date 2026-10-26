//! The live inputs one dispatch run reads: South's orchestration, pace-control's samples and
//! placement's quota verdicts, each host's resident sessions, fleet roster and open gates,
//! NorthThreads claims, the dispatch record and the open issues of the staffed repositories.
use super::issues::{self, Issue, IssueRef, OWNER};
use super::plan::Cloud;
use super::record;
use super::slots::{Account, Host, Room};
use crate::fleet::model::ProviderName;
use crate::fleet::roster::{Roster, Standing};
use crate::pace::exhaustion;
use crate::ops::pacing::placement::{self, gate};
use crate::platform::process::run_within;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;
use std::time::Duration;
use toml_edit::{DocumentMut, Item};

pub(crate) const REPOS: [&str; 2] = ["smashcraft", "wisp"];
const PACE_FRESH_S: f64 = 15.0 * 60.0;
const MIRROR_FRESH_S: f64 = 10.0 * 60.0;
const WEEK_H: f64 = 7.0 * 24.0;
const PROBE: Duration = Duration::from_secs(30);
const RUNNING: [&str; 3] = ["starting", "working", "idle"];
static SESSION_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(sc|smashcraft|w|wisp)-(\d+)[a-z]?(?:-|$)").expect("session pattern"));

pub(crate) struct Survey {
    pub here: String,
    pub cfg: DocumentMut,
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
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Session {
    pub name: String,
    pub item: Option<IssueRef>,
    /// The machine it runs on, when the registry that launched it names one.
    pub machine: Option<String>,
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn host_table<'a>(cfg: &'a DocumentMut, name: &str) -> Option<&'a Item> {
    cfg.get("hosts")?.get(name)
}

/// The issue a session's name starts with: `sc-277-d1435` holds smashcraft#277.
fn named_item(name: &str) -> Option<IssueRef> {
    let found = SESSION_ITEM.captures(name)?;
    let repo = if found[1].starts_with('s') { "smashcraft" } else { "wisp" };
    Some(IssueRef::new(repo, found[2].parse().ok()?))
}

/// The running sessions in one registry's `north fleet list --json`.
pub(crate) fn running_sessions(listing: &Value) -> Vec<Session> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| RUNNING.contains(&session["status"].as_str().unwrap_or_default()))
        .map(|session| {
            let name = session["name"].as_str().unwrap_or_default().to_owned();
            let item = session["task_ref"].as_str().and_then(IssueRef::parse).or_else(|| named_item(&name));
            Session { machine: session["machine"].as_str().map(str::to_owned), name, item }
        })
        .collect()
}

/// The issues the open gates in one host's `north gate list --json` name, in the question or by
/// the asking session's name, each with the gate that holds it.
pub(crate) fn gate_rows(listing: &Value, host: &str) -> Vec<(IssueRef, String)> {
    let mut found = Vec::new();
    for gate in listing.as_array().into_iter().flatten().filter(|gate| gate["closed_ms"].is_null()) {
        let session = gate["session"].as_str().unwrap_or_default();
        let repo = named_item(session).map_or_else(String::new, |item| item.repo);
        let question = gate["question"].as_str().unwrap_or_default();
        let why = format!("open north gate {} on {host} from {session}, for Tom", gate["id"]);
        let mut items: Vec<IssueRef> = issues::references(question, &repo).into_iter().filter(|item| !item.repo.is_empty()).chain(named_item(session)).collect();
        items.sort();
        items.dedup();
        found.extend(items.into_iter().map(|item| (item, why.clone())));
    }
    found
}

fn remote_json(ssh: Option<&str>, command: &[&str]) -> Result<Value, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let north = north.to_string_lossy();
    let output = match ssh {
        None => run_within(&north, command, PROBE),
        Some(destination) => {
            let mut words = vec!["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north"];
            words.extend(command);
            run_within("ssh", &words, PROBE)
        }
    };
    let output = output.ok_or("timed out")?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("failed").to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

/// One `north` JSON listing from this host and from every ssh host, each with the host it came from.
fn listings(cfg: &DocumentMut, here: &str, command: &[&str], notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let remote = gate::machines(cfg).into_iter().filter(|machine| machine.name != here).map(|machine| (machine.name, Some(machine.ssh)));
    let mut found = Vec::new();
    for (host, ssh) in std::iter::once((here.to_owned(), None)).chain(remote) {
        match remote_json(ssh.as_deref(), command) {
            Ok(listing) => found.push((host, listing)),
            Err(why) => notes.push(format!("{host} north {} unread: {why}", command[..2].join(" "))),
        }
    }
    found
}

/// A listing the laptop copies to `KIND-HOST.json` in `directory`, since no host here reaches it
/// over ssh; an old copy still holds its issues.
pub(crate) fn mirrored(directory: &std::path::Path, kind: &str, host: &str, at: f64, notes: &mut Vec<String>) -> Value {
    let path = directory.join(format!("{kind}-{host}.json"));
    let listing = std::fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|text| serde_json::from_str::<Value>(&text).map_err(|error| error.to_string()));
    let copied = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
    match listing {
        Ok(listing) => {
            if let Some(age) = copied.map(|copied| at - copied.as_secs_f64()).filter(|age| *age > MIRROR_FRESH_S) {
                notes.push(format!("{host} {kind} list copied {:.0} min ago", age / 60.0));
            }
            listing
        }
        Err(why) => {
            notes.push(format!("{host} {kind} list unread: {}: {why}", path.display()));
            Value::Null
        }
    }
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
    Host { name: name.into(), cap, running }
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

/// An account's plan size in Max 5x units from `[accounts.PROVIDER] plans`: `max-Nx` is N/5 and `pro` a fifth.
pub(crate) fn plan(cfg: &DocumentMut, account: &str) -> Result<f64, String> {
    let (provider, label) = account.split_once(':').unwrap_or((account, ""));
    let named = cfg.get("accounts").and_then(|accounts| accounts.get(provider)).and_then(|policy| policy.get("plans")).and_then(|plans| plans.get(label)).and_then(Item::as_str);
    let plan = named.ok_or_else(|| format!("{account} has no [accounts.{provider}] plans entry, so dispatch sizes it as max-5x"))?;
    let multiple = if plan == "pro" { Some(1.0) } else { plan.strip_prefix("max-").and_then(|rest| rest.strip_suffix('x')).and_then(|times| times.parse::<f64>().ok()) };
    multiple.filter(|times| *times > 0.0).map(|times| times / 5.0).ok_or_else(|| format!("{account} plan {plan} is neither pro nor max-Nx, so dispatch sizes it as max-5x"))
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

fn accounts(cfg: &DocumentMut, roster: &Roster, here: &str, at: f64, notes: &mut Vec<String>) -> Vec<Account> {
    let pace = placement::pace_state("pace-control.json");
    let usage = placement::pace_state("usage-gate.json");
    pace["accounts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, control)| {
            let sampled = usage["claude"]["controls"].get(name).filter(|_| name.starts_with("claude:"));
            let verdict = sampled.map(|usage| gate::verdict(name, usage));
            let plan = match sampled.map(|_| plan(cfg, name)) {
                Some(Ok(plan)) => plan,
                Some(Err(why)) => {
                    notes.push(why);
                    1.0
                }
                None => 1.0,
            };
            Account {
                name: name.clone(),
                hosts: signed_in(cfg, roster, name, here),
                plan,
                room: sampled.map_or(Room { five_hour: 0.0, week: 0.0, week_reset_h: WEEK_H }, |usage| room(usage, at)),
                spending: control["target"]["by"].as_str().and_then(crate::pace::target::seconds).is_some_and(|by| by > at),
                refusal: refusal(name, control, verdict.as_ref(), at),
            }
        })
        .collect()
}

/// NorthThreads claims from this host's ledger and every ssh host's, since a host whose ledger is
/// local records its workers' claims there.
fn claims(cfg: &DocumentMut, here: &str, notes: &mut Vec<String>) -> Vec<(IssueRef, String)> {
    let remote = gate::machines(cfg).into_iter().filter(|machine| machine.name != here).map(|machine| (machine.name, Some(machine.ssh)));
    let mut found = Vec::new();
    for (host, ssh) in std::iter::once((here.to_owned(), None)).chain(remote) {
        let output = match &ssh {
            None => run_within("threads", &["list"], PROBE),
            Some(destination) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "threads", "list"], PROBE),
        };
        match output.filter(|output| output.status.success()) {
            Some(output) => found.extend(claim_rows(&String::from_utf8_lossy(&output.stdout), &host)),
            None => notes.push(format!("{host} threads list unread, so its claims are unknown")),
        }
    }
    found
}

/// The holders in `threads list` output, whose first line is its header.
pub(crate) fn claim_rows(listing: &str, host: &str) -> Vec<(IssueRef, String)> {
    listing
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let item = IssueRef::parse(columns.next()?)?;
            Some((item, format!("threads claim by {} on {host}", columns.next()?)))
        })
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

/// The sessions this registry runs on other machines, which their resident counts miss, and the
/// holds every running session makes.
pub(crate) fn tally(sessions: &[(Session, String)]) -> (BTreeMap<String, i64>, BTreeMap<IssueRef, String>) {
    let (mut elsewhere, mut holds) = (BTreeMap::new(), BTreeMap::new());
    for (session, registry) in sessions {
        if let Some(machine) = session.machine.as_ref().filter(|machine| *machine != registry) {
            *elsewhere.entry(machine.clone()).or_insert(0) += 1;
        }
        if let Some(item) = &session.item {
            holds.insert(item.clone(), format!("fleet session {} on {registry}", session.name));
        }
    }
    (elsewhere, holds)
}

/// The laptop's resident sessions: those its copied fleet list shows, or dispatch's recent launches
/// there when they are more, since a fresh launch may not show yet.
pub(crate) fn laptop_running(sessions: &[(Session, String)], laptop: &str, dispatched: i64) -> i64 {
    let listed = sessions.iter().filter(|(_, host)| host == laptop).count();
    i64::try_from(listed).unwrap_or(i64::MAX).max(dispatched)
}

pub(crate) fn survey(roster: &Roster) -> Result<Survey, String> {
    let cfg = placement::config()?;
    let here = crate::platform::host::name();
    let at = placement::now();
    let mut notes = Vec::new();
    let laptop = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter()).find(|(name, table)| {
        *name != here && table.get("ssh").is_none() && strings(table.get("serves")).iter().any(|need| need == "laptop")
    });
    let laptop = laptop.map(|(name, _)| name.to_owned());
    let mut fleets = listings(&cfg, &here, &["fleet", "list", "--json"], &mut notes);
    let mut gates = listings(&cfg, &here, &["gate", "list", "--json"], &mut notes);
    if let Some(name) = &laptop {
        fleets.push((name.clone(), mirrored(&record::directory(), "fleet", name, at, &mut notes)));
        gates.push((name.clone(), mirrored(&record::directory(), "gates", name, at, &mut notes)));
    }
    let sessions: Vec<(Session, String)> = fleets.iter().flat_map(|(host, listing)| running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    let (elsewhere, mut holds) = tally(&sessions);
    let (issues, open) = github::open_issues(&mut notes);
    let records = record::read();
    holds.extend(claims(&cfg, &here, &mut notes));
    holds.extend(record::holds(&records, at));
    holds.extend(gates.iter().flat_map(|(host, listing)| gate_rows(listing, host)));
    let on_laptop = laptop.as_deref().map_or(0, |name| laptop_running(&sessions, name, record::recent_on(&records, name, &open, at)));
    let names: Vec<String> = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter().map(|(name, _)| name.to_owned())).collect();
    let hosts = names.iter().map(|name| host(&cfg, name, &here, elsewhere.get(name).copied().unwrap_or(0), on_laptop)).collect();
    let accounts = accounts(&cfg, roster, &here, at, &mut notes);
    if accounts.is_empty() {
        notes.push("pace-control.json names no account, so no slot is free".into());
    }
    let cloud = cloud(&cfg, &here, at);
    let cloud_running = record::recent_on(&records, super::cloud::HOST, &open, at);
    Ok(Survey { here, cfg, hosts, accounts, issues, open, holds, cloud, laptop, cloud_running, notes, at })
}

mod github {
    use super::*;

    const QUERY: &str = "query($owner: String!, $name: String!, $cursor: String) { repository(owner: $owner, name: $name) { issues(states: OPEN, first: 50, after: $cursor) { pageInfo { hasNextPage endCursor } nodes { number title body labels(first: 30) { nodes { name } } blockedBy(first: 30) { nodes { number state repository { name } } } comments(last: 1) { nodes { body } } } } } }";

    pub(crate) type OpenIssues = (Vec<Issue>, BTreeSet<IssueRef>);

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
    pub(crate) fn open_issues(notes: &mut Vec<String>) -> OpenIssues {
        let mut found = Vec::new();
        for repo in REPOS {
            match repository(repo) {
                Ok(issues) => found.extend(issues),
                Err(why) => notes.push(format!("{repo} issues unread: {why}")),
            }
        }
        let mut open: BTreeSet<IssueRef> = found.iter().map(Issue::reference).collect();
        let planned = issues::planned_predecessors(&found);
        let named = found.iter().flat_map(|issue| issues::predecessors(issue).unwrap_or_default()).chain(planned.into_values().flatten());
        let elsewhere: BTreeSet<IssueRef> = named.filter(|reference| !REPOS.contains(&reference.repo.as_str())).collect();
        for reference in elsewhere {
            if still_open(&reference) != Some(false) {
                open.insert(reference);
            }
        }
        (found, open)
    }
}
