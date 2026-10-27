//! GitHub and NorthThreads are foreign sources; North keeps no work-item database.
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::time::Duration;
use serde::Deserialize;

use crate::error::{NorthError, NorthResult};
use crate::platform::sqlite::{self, Access};
use crate::platform::{github, process};

pub const HELP: &str = "Work items\n\n  north work list\n  north work search TEXT\n  north work show REPO#N\n  north work claim REPO#N [--by NAME] [--eta MINUTES]\n  north work release REPO#N [--to NAME]\n  north work need REPO#N BLOCKER#N\n  north work unneed REPO#N BLOCKER#N\n  north work close REPO#N --comment RESULT\n\n\nGitHub owns issues; NorthThreads records holders, handoffs and runs.";

async fn command(program: &str, arguments: &[String]) -> NorthResult<String> {
    let result = if program == "gh" {
        github::run_async(arguments).await?
    } else {
        process::run_killing_on_drop(program, arguments).await?
    };
    if !result.status.success() {
        return Err(NorthError::Configuration(format!("{program}: {}", String::from_utf8_lossy(&result.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

async fn ledger_rows(query: &str) -> NorthResult<serde_json::Value> {
    let db = env::var_os("THREADS_DB").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state/threads/threads.db")
    });
    let query = query.to_owned();
    let rows = tokio::task::spawn_blocking(move || sqlite::rows(&db, Access::ReadOnly, Duration::ZERO, &query))
        .await
        .map_err(|error| NorthError::State(error.to_string()))??;
    Ok(serde_json::Value::Array(rows))
}

#[derive(Deserialize)]
struct Cache {
    repo: String,
    issues: String,
}
#[derive(Deserialize)]
struct Holder {
    item: String,
    owner: String,
    clock_min: i64,
    eta_min: Option<i64>,
    standing: String,
}
#[derive(Deserialize, Default)]
struct Issue {
    title: String,
    #[serde(default)]
    blockers: Vec<Blocker>,
}
#[derive(Deserialize)]
struct Blocker {
    state: String,
}
#[derive(Deserialize)]
struct SearchIssue {
    number: u64,
    title: String,
    state: String,
    repository: Repository,
}
#[derive(Deserialize)]
struct Repository {
    name: String,
}

pub struct WorkRow {
    pub reference: String,
    pub title: String,
    pub state: String,
    pub blocked: bool,
    pub holder: String,
    pub clock: String,
    pub eta: String,
}

pub enum ResultView {
    List(Vec<WorkRow>),
    Document { title: String, text: String },
}

fn request(arguments: &[String]) -> NorthResult<(&'static str, String)> {
    match arguments {
        [] => Ok(("work-list", String::new())),
        [verb] if verb == "list" => Ok(("work-list", String::new())),
        [verb, rest @ ..] if verb == "search" && !rest.is_empty() => Ok(("work-search", rest.join(" "))),
        [verb, reference, rest @ ..] => {
            let action = action(verb).ok_or_else(|| NorthError::Usage(HELP.into()))?;
            let payload = std::iter::once(reference.as_str()).chain(rest.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
            Ok((action, payload))
        }
        _ => Err(NorthError::Usage(HELP.into())),
    }
}

fn action(verb: &str) -> Option<&'static str> {
    Some(match verb {
        "show" => "work-show",
        "claim" => "work-claim",
        "release" => "work-release",
        "need" => "work-need",
        "unneed" => "work-unneed",
        "close" => "work-close",
        _ => return None,
    })
}

fn row_status(row: &WorkRow) -> &'static str {
    if row.state == "closed" { "closed" } else if row.blocked { "blocked" } else if row.holder.is_empty() { "ready" } else { "held" }
}

fn holder(row: &WorkRow) -> String {
    if row.holder.is_empty() { "No holder".into() } else { format!("{} · {} held / {} ETA", row.holder, row.clock, row.eta) }
}

/// The holders `threads list --json` reports as live; a void or stale claim holds nothing.
fn live_holders(listing: &str) -> NorthResult<BTreeMap<String, Holder>> {
    let holders: Vec<Holder> = serde_json::from_str(listing)?;
    Ok(holders.into_iter().filter(|holder| holder.standing == "live").map(|holder| (holder.item.clone(), holder)).collect())
}

async fn rows(query: Option<&str>) -> NorthResult<Vec<WorkRow>> {
    // Refresh through NorthThreads so its repository map and cache remain the only source.
    command("threads", &["ready".into()]).await?;
    let caches: Vec<Cache> = serde_json::from_value(ledger_rows("SELECT repo, issues FROM gh_cache ORDER BY repo").await?)?;
    let holders = live_holders(&command("threads", &["list".into(), "--json".into()]).await?)?;
    let mut issues = BTreeMap::new();
    for cache in &caches {
        let entries: BTreeMap<String, Issue> = serde_json::from_str(&cache.issues)?;
        for (number, issue) in entries { issues.insert(format!("{}#{number}", cache.repo), issue); }
    }
    let entries = if let Some(query) = query {
        let mut args = vec!["search".into(), "issues".into(), query.into(), "--limit".into(), "100".into(), "--json".into(), "number,title,state,repository".into()];
        for cache in &caches { args.extend(["--repo".into(), format!("tompassarelli/{}", cache.repo)]); }
        let hits: Vec<SearchIssue> = serde_json::from_str(&command("gh", &args).await?)?;
        hits.into_iter().map(|hit| (format!("{}#{}", hit.repository.name, hit.number), hit.title, hit.state.to_lowercase())).collect::<Vec<_>>()
    } else {
        issues.iter().map(|(reference, issue)| (reference.clone(), issue.title.clone(), "open".into())).collect()
    };
    Ok(entries.into_iter().map(|(reference, title, state)| {
        let holder = holders.get(&reference);
        let blocked = issues.get(&reference).is_some_and(|issue| issue.blockers.iter().any(|blocker| blocker.state == "OPEN"));
        WorkRow { title, state, blocked,
            holder: holder.map(|holder| holder.owner.clone()).unwrap_or_default(),
            clock: holder.map(|holder| format!("{}m", holder.clock_min)).unwrap_or_default(),
            eta: holder.and_then(|holder| holder.eta_min).map(|eta| format!("{eta}m")).unwrap_or_else(|| "?".into()),
            reference,
        }
    }).collect())
}

async fn issue_ref(reference: &str) -> NorthResult<(String, String)> {
    let (repo, number) = reference.split_once('#').ok_or_else(|| NorthError::Usage("Use an issue reference like smashcraft#247".into()))?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(NorthError::Usage("Use an issue reference like smashcraft#247".into()));
    }
    let repo = repo.strip_prefix("tompassarelli/").unwrap_or(repo);
    let repo = if repo == "nixos-config" { "firn" } else { repo };
    let known = ledger_rows("SELECT repo FROM gh_cache").await?;
    if !known.as_array().is_some_and(|rows| rows.iter().any(|row| row["repo"].as_str() == Some(repo))) {
        return Err(NorthError::Usage(format!("Unknown work repository {repo}; run north work list first")));
    }
    Ok((format!("tompassarelli/{repo}"), number.into()))
}

async fn show(reference: &str) -> NorthResult<ResultView> {
    let (repo, number) = issue_ref(reference).await?;
    let issue: serde_json::Value = serde_json::from_str(&command("gh", &["issue".into(), "view".into(), number, "--repo".into(), repo, "--json".into(), "title,body,url,state".into()]).await?)?;
    let history = command("threads", &["show".into(), reference.into()]).await?;
    let body = issue["body"].as_str().unwrap_or("");
    Ok(ResultView::Document { title: format!("{reference} — {}", issue["title"].as_str().unwrap_or("")), text: format!("{history}\n{}\n\n{body}", issue["url"].as_str().unwrap_or("")) })
}

pub async fn execute(action: &str, payload: &str) -> NorthResult<ResultView> {
    match action {
        "work-list" => Ok(ResultView::List(rows(None).await?)),
        "work-search" if !payload.trim().is_empty() => Ok(ResultView::List(rows(Some(payload)).await?)),
        "work-show" => show(payload.trim()).await,
        "work-claim" | "work-release" | "work-need" | "work-unneed" => {
            let mut args: Vec<String> = payload.split_whitespace().map(str::to_owned).collect();
            let reference = args.first().cloned().ok_or_else(|| NorthError::Usage(HELP.into()))?;
            let (repo, number) = issue_ref(&reference).await?;
            let verb = action.strip_prefix("work-").unwrap();
            args.insert(0, verb.into());
            let result = command("threads", &args).await?;
            command("gh", &["issue".into(), "comment".into(), number, "--repo".into(), repo, "--body".into(), format!("North work {verb}: {}", result.trim())]).await?;
            show(&reference).await
        }
        "work-close" => {
            let (reference, rest) = payload.trim().split_once(char::is_whitespace).ok_or_else(|| NorthError::Usage(HELP.into()))?;
            let comment = rest.trim().strip_prefix("--comment ").filter(|comment| !comment.trim().is_empty()).ok_or_else(|| NorthError::Usage("Close with the measured result: north work close REPO#N --comment RESULT".into()))?;
            let (repo, number) = issue_ref(reference).await?;
            command("gh", &["issue".into(), "close".into(), number, "--repo".into(), repo, "--comment".into(), comment.into()]).await?;
            show(reference).await
        }
        _ => Err(NorthError::Usage(HELP.into())),
    }
}

pub async fn run(arguments: &[String]) -> NorthResult<()> {
    if arguments.first().is_some_and(|arg| matches!(arg.as_str(), "help" | "--help" | "-h")) {
        println!("{HELP}"); return Ok(());
    }
    let (action, payload) = request(arguments)?;
    let view = execute(action, &payload).await?;
    match view {
        ResultView::List(rows) => {
            println!("Item\tTitle\tStatus\tHolder / time / ETA");
            for row in rows {
                println!("{}\t{}\t{}\t{}", row.reference, row.title, row_status(&row), holder(&row));
            }
        }
        ResultView::Document { title, text } => println!("{title}\n\n{text}"),
    }
    Ok(())
}
