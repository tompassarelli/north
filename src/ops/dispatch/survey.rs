//! The live inputs one dispatch run reads: South's orchestration, pace-control's allowances and
//! placement's quota verdicts, each host's resident sessions and fleet roster, NorthThreads claims,
//! the dispatch record and the open issues of the staffed repositories.
use super::issues::{self, Issue, IssueRef, OWNER};
use super::plan::Cloud;
use super::record;
use super::slots::{Account, Host};
use crate::fleet::roster::Roster;
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
const PROBE: Duration = Duration::from_secs(30);
const RUNNING: [&str; 3] = ["starting", "working", "idle"];
static SESSION_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(sc|smashcraft|w|wisp)-(\d+)(?:-|$)").expect("session pattern"));

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
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Session {
    pub name: String,
    pub account: String,
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

/// The running sessions in one registry's `north fleet list --json`.
pub(crate) fn running_sessions(listing: &Value) -> Vec<Session> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| RUNNING.contains(&session["status"].as_str().unwrap_or_default()))
        .map(|session| {
            let name = session["name"].as_str().unwrap_or_default().to_owned();
            let item = session["task_ref"].as_str().and_then(IssueRef::parse).or_else(|| {
                let found = SESSION_ITEM.captures(&name)?;
                let repo = if found[1].starts_with('s') { "smashcraft" } else { "wisp" };
                Some(IssueRef::new(repo, found[2].parse().ok()?))
            });
            Session {
                account: format!("{}:{}", session["provider"].as_str().unwrap_or_default(), session["account"].as_str().unwrap_or_default()),
                machine: session["machine"].as_str().map(str::to_owned),
                name,
                item,
            }
        })
        .collect()
}

fn fleet_listing(ssh: Option<&str>) -> Result<Value, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let north = north.to_string_lossy();
    let output = match ssh {
        None => run_within(&north, &["fleet", "list", "--json"], PROBE),
        Some(destination) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north", "fleet", "list", "--json"], PROBE),
    };
    let output = output.ok_or("timed out")?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("failed").to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

/// Running sessions in this host's registry and in every ssh host's, each with the host whose
/// resident count already includes it.
fn sessions(cfg: &DocumentMut, here: &str, notes: &mut Vec<String>) -> Vec<(Session, String)> {
    let mut found = Vec::new();
    let remote = gate::machines(cfg).into_iter().filter(|machine| machine.name != here).map(|machine| (machine.name, Some(machine.ssh)));
    for (registry, ssh) in std::iter::once((here.to_owned(), None)).chain(remote) {
        match fleet_listing(ssh.as_deref()) {
            Ok(listing) => found.extend(running_sessions(&listing).into_iter().map(|session| (session, registry.clone()))),
            Err(why) => notes.push(format!("{registry} fleet list unread: {why}")),
        }
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

fn accounts(cfg: &DocumentMut, roster: &Roster, here: &str, running: &BTreeMap<String, i64>, at: f64) -> Vec<Account> {
    let pace = placement::pace_state("pace-control.json");
    let verdicts = gate::verdicts(&placement::pace_state("usage-gate.json"), roster);
    pace["accounts"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, control)| {
            let verdict = verdicts.iter().find(|verdict| verdict.account == *name);
            let age = at - control["at"].as_f64().unwrap_or(0.0);
            let refusal = if age > PACE_FRESH_S {
                Some(format!("pace-control.json sampled {:.0} min ago", age / 60.0))
            } else {
                verdict.filter(|verdict| !verdict.available).map(|verdict| verdict.line.clone())
            };
            Account {
                name: name.clone(),
                allowed: control["allowed_workers"].as_i64(),
                running: running.iter().filter(|(session, _)| *session == name || session.split(':').next() == Some(name)).map(|(_, count)| count).sum(),
                hosts: account_hosts(cfg, name, here),
                headroom: verdict.map_or(0.0, |verdict| verdict.headroom),
                refusal,
            }
        })
        .collect()
}

fn claims(notes: &mut Vec<String>) -> Vec<(IssueRef, String)> {
    let Some(output) = run_within("threads", &["list"], PROBE).filter(|output| output.status.success()) else {
        notes.push("threads list unread, so claims are unknown".into());
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let item = IssueRef::parse(columns.next()?)?;
            Some((item, format!("threads claim by {}", columns.next()?)))
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

/// Sessions per account and the holds they make, the sessions this registry runs on other
/// machines, which their resident counts miss.
fn tally(sessions: &[(Session, String)]) -> (BTreeMap<String, i64>, BTreeMap<String, i64>, BTreeMap<IssueRef, String>) {
    let (mut per_account, mut elsewhere, mut holds) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    for (session, registry) in sessions {
        *per_account.entry(session.account.clone()).or_insert(0) += 1;
        if let Some(machine) = session.machine.as_ref().filter(|machine| *machine != registry) {
            *elsewhere.entry(machine.clone()).or_insert(0) += 1;
        }
        if let Some(item) = &session.item {
            holds.insert(item.clone(), format!("fleet session {} on {registry}", session.name));
        }
    }
    (per_account, elsewhere, holds)
}

pub(crate) fn survey(roster: &Roster) -> Result<Survey, String> {
    let cfg = placement::config()?;
    let here = crate::platform::host::name();
    let at = placement::now();
    let mut notes = Vec::new();
    let sessions = sessions(&cfg, &here, &mut notes);
    let (per_account, elsewhere, mut holds) = tally(&sessions);
    let (issues, open) = github::open_issues(&mut notes);
    let records = record::read();
    holds.extend(claims(&mut notes));
    holds.extend(record::holds(&records, at));
    let laptop = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter()).find(|(name, table)| {
        *name != here && table.get("ssh").is_none() && strings(table.get("serves")).iter().any(|need| need == "laptop")
    });
    let laptop = laptop.map(|(name, _)| name.to_owned());
    let laptop_running = laptop.as_deref().map_or(0, |name| record::recent_on(&records, name, &open, at));
    let names: Vec<String> = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter().map(|(name, _)| name.to_owned())).collect();
    let hosts = names.iter().map(|name| host(&cfg, name, &here, elsewhere.get(name).copied().unwrap_or(0), laptop_running)).collect();
    let accounts = accounts(&cfg, roster, &here, &per_account, at);
    let cloud = cloud(&cfg, &here, at);
    Ok(Survey { here, cfg, hosts, accounts, issues, open, holds, cloud, laptop, notes })
}

mod github {
    use super::*;

    const QUERY: &str = "query($owner: String!, $name: String!, $cursor: String) { repository(owner: $owner, name: $name) { issues(states: OPEN, first: 50, after: $cursor) { pageInfo { hasNextPage endCursor } nodes { number title body labels(first: 30) { nodes { name } } blockedBy(first: 30) { nodes { number state repository { name } } } } } } }";

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
