use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use regex::Regex;

use super::audit_rows::{Facts, Row, fault, gc_problems, leftover_table, open_table, row};
use super::evidence::{IssueState, cited_issues, issue_state, item_ref, origin_repo};
use super::exec::{api, command, git};
use super::liveness::{open_pull_heads, pending_autoland};
use super::remote::{active_runs, remote_branches};
use super::state::write_atomically;
use north_github_mirror::store::{self, Mirror};
use north_github_mirror::read;
use north_core::foundation::clock::Timestamp;

const USAGE: &str = "usage: north repo audit [--repo NAME]... [--alert]";
const LANDING: &str = "claude/land-";
const FRESH_MS: i64 = 4 * 3_600_000;
const OWNER: &str = "tompassarelli";
const SINCE: &str = "2026-01-01";
const LISTED: usize = 12;
const REPORT: &str = ".local/state/agents/repo-audit/latest.txt";
const OPEN: &str = "Open this report: run `north repo audit`, or read ~/.local/state/agents/repo-audit/latest.txt (rewritten each night at 3:41 AM)";

#[derive(Debug, Default, PartialEq)]
pub(super) struct Findings {
    pub(super) families: BTreeMap<String, Vec<String>>,
    pub(super) strays: Vec<String>,
    pub(super) unreleased: Vec<String>,
}

impl Findings {
    pub(super) fn clean(&self) -> bool {
        self.families.is_empty() && self.strays.is_empty() && self.unreleased.is_empty()
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct Repo {
    pub(super) name: String,
    pub(super) default: String,
}

struct Audited {
    rows: Vec<Row>,
    tags: Findings,
}

pub(super) fn select_repos(listing: &Value, only: &[String]) -> Vec<Repo> {
    let mut repos: Vec<Repo> = listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|repo| repo["isArchived"].as_bool() != Some(true) && repo["pushedAt"].as_str().is_some_and(|at| at >= SINCE))
        .filter(|repo| repo["isFork"].as_bool() != Some(true) || repo["parent"]["owner"]["login"].as_str() == Some(OWNER))
        .filter_map(|repo| {
            let name = repo["name"].as_str()?.to_string();
            let default = repo["defaultBranchRef"]["name"].as_str().unwrap_or("main").to_string();
            (only.is_empty() || only.contains(&name)).then_some(Repo { name, default })
        })
        .collect();
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    repos
}

pub(super) fn ref_names(pages: &Value, prefix: &str) -> Vec<String> {
    let rows = pages.as_array().into_iter().flatten().flat_map(|page| page.as_array().into_iter().flatten());
    rows.filter_map(|row| row["ref"].as_str()?.strip_prefix(prefix).map(str::to_string)).collect()
}

pub(super) fn is_release_tag(tag: &str) -> bool {
    tag.strip_prefix('v').and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_ascii_digit())
}

fn family(tag: &str) -> Option<String> {
    let at = tag.find(['-', '/'])?;
    let prefix = &tag[..at];
    let plain = !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'));
    plain.then(|| format!("{prefix}{}*", &tag[at..=at]))
}

pub(super) fn live_ticket(branch: &str, open_pull: bool, active: bool, pending: bool, age_ms: i64) -> bool {
    branch.starts_with(LANDING) && (open_pull || active || pending || age_ms < FRESH_MS)
}

pub(super) fn classify_tags(tags: &[String], released: &HashSet<String>) -> Findings {
    let mut found = Findings::default();
    for tag in tags {
        if is_release_tag(tag) {
            if !released.contains(tag) {
                found.unreleased.push(tag.clone());
            }
        } else if let Some(label) = family(tag) {
            found.families.entry(label).or_default().push(tag.clone());
        } else {
            found.strays.push(tag.clone());
        }
    }
    found.strays.sort();
    found.unreleased.sort();
    found
}

fn listed(label: &str, names: &[String]) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    let shown: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
    let more = names.len().saturating_sub(LISTED);
    vec![format!("  {label} ({}): {}{}", names.len(), shown.join(" "), if more > 0 { format!(" (+{more} more)") } else { String::new() })]
}

pub(super) fn render_tags(name: &str, found: &Findings) -> Vec<String> {
    let mut lines = vec![format!("{name}:")];
    for (label, tags) in &found.families {
        lines.extend(listed(&format!("non-release tags {label}"), tags));
    }
    lines.extend(listed("non-release stray tags", &found.strays));
    lines.extend(listed("v* tags without a GitHub release", &found.unreleased));
    lines
}

pub(super) fn tag_headline(name: &str, found: &Findings) -> String {
    let families: usize = found.families.values().map(Vec::len).sum();
    format!("{name}: {} non-release tags, {} v-tags without release", families + found.strays.len(), found.unreleased.len())
}

pub(super) fn pages_source(response: &Value) -> Option<String> {
    response["source"]["branch"].as_str().filter(|branch| !branch.is_empty()).map(str::to_string)
}

pub(super) fn audited_branch(branch: &str, default: &str, pages: Option<&str>) -> bool {
    branch != default && pages != Some(branch)
}

fn pages_branch(repo: &str) -> Result<Option<String>, String> {
    let ran = command(&["gh", "api", &format!("repos/{OWNER}/{repo}/pages")]);
    if ran.ok {
        let response: Value = serde_json::from_str(&ran.out).map_err(|error| error.to_string())?;
        return Ok(pages_source(&response));
    }
    if ran.err.contains("404") || ran.err.contains("Not Found") {
        return Ok(None);
    }
    Err(format!("Pages source unavailable: {}", ran.last_err_line()))
}

fn released_tags(repo: &str) -> Result<HashSet<String>, String> {
    let miss = || Ok(api(&format!("repos/{OWNER}/{repo}/releases?per_page=100"))?.as_array().into_iter().flatten().filter_map(Value::as_array).flatten().cloned().collect());
    let releases = read::listed(&Mirror::local(), read::now(), repo, store::RELEASES, miss)?;
    let releases = releases.iter();
    Ok(releases.filter(|release| release["draft"].as_bool() != Some(true)).filter_map(|release| release["tag_name"].as_str().map(str::to_string)).collect())
}

fn running_workers() -> Result<HashMap<(String, String), String>, String> {
    let listed = command(&["north", "fleet", "list", "--json"]);
    if !listed.ok {
        return Err("Fleet sessions unavailable".into());
    }
    let sessions: Value = serde_json::from_str(&listed.out).map_err(|error| error.to_string())?;
    let mut workers = HashMap::new();
    for session in sessions.as_array().into_iter().flatten().filter(|session| session["status"].as_str() != Some("exited")) {
        let (Some(cwd), Some(name)) = (session["cwd"].as_str().map(Path::new), session["name"].as_str()) else { continue };
        let Some(container) = cwd.ancestors().find(|dir| dir.join("main/.git").exists()) else { continue };
        let Some(repo) = origin_repo(&container.join("main")) else { continue };
        let current = git(cwd, &["branch", "--show-current"]);
        if current.ok && !current.out.is_empty() {
            workers.insert((repo, current.out), name.to_string());
        }
    }
    Ok(workers)
}

pub(super) fn named_issues(messages: &str) -> Vec<String> {
    let item = Regex::new(r"(?m)^\s*Item:\s*([\w.-]*#\d+)").expect("item pattern");
    let mut found = cited_issues(messages);
    for captured in item.captures_iter(messages) {
        let issue = captured[1].to_string();
        if !found.contains(&issue) {
            found.push(issue);
        }
    }
    found
}

fn compare(repo: &Repo, branch: &str) -> Result<(bool, Vec<String>), String> {
    let ran = command(&["gh", "api", &format!("repos/{OWNER}/{}/compare/{}...{branch}", repo.name, repo.default)]);
    if !ran.ok {
        return Err(format!("compare {branch} failed: {}", ran.last_err_line()));
    }
    let facts: Value = serde_json::from_str(&ran.out).map_err(|error| error.to_string())?;
    let messages: String = facts["commits"].as_array().into_iter().flatten().filter_map(|commit| commit["commit"]["message"].as_str()).map(|message| format!("{message}\n")).collect();
    Ok((facts["ahead_by"].as_u64() == Some(0), named_issues(&messages)))
}

fn audit_repo(repo: &Repo, workers: &HashMap<(String, String), String>, now: i64) -> Result<Audited, String> {
    let name = &repo.name;
    let pages = pages_branch(name)?;
    let nodes: Vec<Value> = remote_branches(name)?.into_iter().filter(|node| node["name"].as_str().is_some_and(|branch| audited_branch(branch, &repo.default, pages.as_deref()))).collect();
    let (pulls, active, active_tips) = if nodes.iter().any(|node| node["name"].as_str().is_some_and(|branch| branch.starts_with(LANDING))) {
        let (active, tips) = active_runs(name)?;
        (open_pull_heads(name)?, active, tips)
    } else {
        Default::default()
    };
    let mut rows = Vec::new();
    for node in &nodes {
        let branch = node["name"].as_str().unwrap_or_default();
        let target = &node["target"];
        let committed = target["committedDate"].as_str().and_then(Timestamp::parse_rfc3339).map_or(now, Timestamp::unix_millis);
        let tip_active = target["oid"].as_str().is_some_and(|oid| active_tips.contains(oid));
        let ticket = live_ticket(branch, pulls.contains(branch), active.contains(branch) || tip_active, pending_autoland(target.get("status")), now - committed);
        let session = workers.get(&(name.clone(), branch.to_string())).cloned();
        let (is_landed, issues) = compare(repo, branch)?;
        rows.push(row(&Facts { repo: name, branch, committed_ms: committed, landed: is_landed, session, ticket, issues }, now));
    }
    let miss = || Ok(api(&format!("repos/{OWNER}/{name}/git/matching-refs/tags/"))?.as_array().into_iter().flatten().filter_map(Value::as_array).flatten().cloned().collect());
    let tags = ref_names(&Value::Array(vec![Value::Array(read::listed(&Mirror::local(), read::now(), name, store::TAGS, miss)?)]), "refs/tags/");
    let released = if tags.iter().any(|tag| is_release_tag(tag)) { released_tags(name)? } else { HashSet::new() };
    Ok(Audited { rows, tags: classify_tags(&tags, &released) })
}

fn parse(arguments: &[String]) -> Option<(Vec<String>, bool)> {
    let (mut repos, mut alert) = (Vec::new(), false);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--repo" => repos.push(words.next()?.clone()),
            "--alert" => alert = true,
            _ => return None,
        }
    }
    Some((repos, alert))
}

fn gc_state(home: &Path) -> (Vec<(&'static str, String)>, Option<i64>) {
    let mut results = Vec::new();
    let unit = "lane-gc.service";
    let ran = command(&["systemctl", "--user", "show", "-p", "Result", "--value", unit]);
    if ran.ok && !ran.out.is_empty() {
        results.push((unit, ran.out));
    }
    let state = std::fs::read_to_string(home.join(".local/state/agents/lane-gc/stale.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let at = state.as_ref().and_then(|state| state["at"].as_str()).and_then(Timestamp::parse_rfc3339).map(Timestamp::unix_millis);
    (results, at)
}

fn alert(text: &str) {
    if !Command::new("supply-alert").args(["normal", text, "Repo hygiene"]).status().is_ok_and(|status| status.success()) {
        eprintln!("repo audit: supply-alert failed");
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((only, send)) = (match arguments.split_first() {
        Some((verb, rest)) if verb == "audit" => parse(rest),
        _ => None,
    }) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let listed = command(&["gh", "repo", "list", OWNER, "--limit", "1000", "--json", "name,isArchived,isFork,pushedAt,defaultBranchRef,parent"]);
    let listing = Some(listed.ok).filter(|ok| *ok).and_then(|_| serde_json::from_str::<Value>(&listed.out).ok());
    let workers = running_workers();
    let (Some(listing), Ok(workers)) = (listing, &workers) else {
        let why = workers.as_ref().err().cloned().unwrap_or_else(|| format!("repository list unavailable: {}", listed.last_err_line()));
        eprintln!("repo audit: {why}");
        if send {
            alert(&format!("repo audit could not gather evidence: {why}"));
        }
        return 1;
    };
    let now = Timestamp::now().unix_millis();
    let repos = select_repos(&listing, &only);
    let (mut rows, mut tag_lines, mut problems) = (Vec::new(), Vec::new(), Vec::new());
    for repo in &repos {
        match audit_repo(repo, workers, now) {
            Ok(audited) => {
                if !audited.tags.clean() {
                    tag_lines.extend(render_tags(&repo.name, &audited.tags));
                    problems.push(tag_headline(&repo.name, &audited.tags));
                }
                rows.extend(audited.rows);
            }
            Err(error) => {
                tag_lines.push(format!("{}: audit failed: {error}", repo.name));
                problems.push(format!("{}: audit failed: {error}", repo.name));
            }
        }
    }
    let mut states: HashMap<String, IssueState> = HashMap::new();
    for issue in rows.iter().filter(|row| !row.leftover).flat_map(|row| row.issues.iter()) {
        states.entry(issue.clone()).or_insert_with(|| issue_state(&item_ref(issue)));
    }
    let faults: Vec<String> = rows.iter().filter_map(|row| fault(row, &states).map(|why| format!("{}:{} ({why})", row.repo, row.branch))).collect();
    if !faults.is_empty() {
        let shown: Vec<&str> = faults.iter().take(LISTED).map(String::as_str).collect();
        problems.insert(0, format!("{} branches need attention: {}", faults.len(), shown.join(" ")));
    }
    let (results, state_at) = gc_state(&home);
    problems.extend(gc_problems(&results, state_at.map(|at| now - at), &rows));
    let leftovers = rows.iter().filter(|row| row.leftover).count();
    let mut report = vec![format!("Landed branches that still exist ({leftovers}); cleanup is broken when one is older than an hour")];
    report.extend(if leftovers == 0 { vec!["  none".to_string()] } else { leftover_table(&rows) });
    report.push(String::new());
    report.push(format!("Open work: unlanded branches on {} active repositories, grouped by the issue they name, oldest first ({}); they stay until they land", repos.len(), rows.len() - leftovers));
    report.extend(open_table(&rows, &states));
    report.push(String::new());
    report.push("Tags".into());
    report.extend(if tag_lines.is_empty() { vec!["  every tag is a v* release with a GitHub release".to_string()] } else { tag_lines });
    report.push(String::new());
    report.push(format!("repo audit: {} repositories audited, {} need attention", repos.len(), problems.len()));
    report.push(OPEN.into());
    let text = report.join("\n") + "\n";
    print!("{text}");
    if only.is_empty() && write_atomically(&home.join(REPORT), &text).is_err() {
        eprintln!("repo audit: could not write {REPORT}");
    }
    if problems.is_empty() {
        return 0;
    }
    if send {
        alert(&format!("{}\n{OPEN}", problems.join("\n")));
    }
    1
}
