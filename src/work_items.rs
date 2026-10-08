//! GitHub and threads are foreign sources; North keeps no work-item database.
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use tokio::process::Command;
use serde::Deserialize;

use crate::clause_state::NorthState;
use crate::error::{NorthError, NorthResult};

pub const HELP: &str = "Work items\n\n  north work list\n  north work search TEXT\n  north work show REPO#N\n  north work claim REPO#N [--by NAME] [--eta MINUTES]\n  north work release REPO#N [--to NAME]\n  north work need REPO#N BLOCKER#N\n  north work unneed REPO#N BLOCKER#N\n  north work close REPO#N --comment RESULT\n\nIn the app use /work followed by the same arguments. Escape closes a list or item.\nGitHub owns issues; threads records holders, handoffs and runs.";

async fn command(program: &str, arguments: &[String]) -> NorthResult<String> {
    let result = Command::new(program).args(arguments).kill_on_drop(true).output().await?;
    if !result.status.success() {
        return Err(NorthError::Configuration(format!("{program}: {}", String::from_utf8_lossy(&result.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

async fn sqlite(query: &str) -> NorthResult<serde_json::Value> {
    let db = env::var_os("THREADS_DB").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state/threads/threads.db")
    });
    let text = command("sqlite3", &["-readonly".into(), "-json".into(), db.to_string_lossy().into_owned(), query.into()]).await?;
    Ok(if text.trim().is_empty() { serde_json::json!([]) } else { serde_json::from_str(&text)? })
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
    clock: i64,
    eta_min: Option<i64>,
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

async fn rows(query: Option<&str>) -> NorthResult<Vec<WorkRow>> {
    // Refresh through threads so its repository map and cache remain the only source.
    command("threads", &["ready".into()]).await?;
    let caches: Vec<Cache> = serde_json::from_value(sqlite("SELECT repo, issues FROM gh_cache ORDER BY repo").await?)?;
    let holders: Vec<Holder> = serde_json::from_value(sqlite("SELECT item, owner, max(0,CAST((julianday('now')-julianday(since))*1440 AS INTEGER)) AS clock, eta_min FROM claims").await?)?;
    let holders: BTreeMap<_, _> = holders.into_iter().map(|holder| (holder.item.clone(), holder)).collect();
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
            clock: holder.map(|holder| format!("{}m", holder.clock)).unwrap_or_default(),
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
    let known = sqlite("SELECT repo FROM gh_cache").await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_6_work_commands_route_without_a_coding_turn() {
        let mut state = NorthState::open().unwrap();
        for (input, action, payload) in [
            ("/work", "work-list", ""),
            ("/work search Archer", "work-search", "Archer"),
            ("/work show smashcraft#247", "work-show", "smashcraft#247"),
            ("/work claim north#7 --by tom --eta 5", "work-claim", "north#7 --by tom --eta 5"),
            ("/work release north#7", "work-release", "north#7"),
            ("/work need north#7 north#6", "work-need", "north#7 north#6"),
            ("/work unneed north#7 north#6", "work-unneed", "north#7 north#6"),
            ("/work close north#7 --comment 2 actions passed", "work-close", "north#7 --comment 2 actions passed"),
        ] {
            state.execute_command(input).unwrap();
            let effect = state.host_effect().unwrap();
            assert_eq!((effect.action(), effect.payload()), (action, payload), "{input}");
            assert!(state.pending_inputs().is_empty(), "{input} must not submit to Codex");
            state.clear_host_effect().unwrap();
        }
    }

    #[test]
    fn spec_6_closed_blocked_held_and_ready_items_show_their_status() {
        let mut state = NorthState::open().unwrap();
        let rows = [
            ("north#1", "open", false, "", "ready"),
            ("north#2", "open", true, "", "blocked"),
            ("north#3", "open", false, "tom", "held"),
            ("smashcraft#247", "closed", false, "", "closed"),
        ].into_iter().map(|(reference, status, blocked, holder, _)| WorkRow {
            reference: reference.into(), title: "Archer".into(), state: status.into(), blocked,
            holder: holder.into(), clock: "3m".into(), eta: "5m".into(),
        }).collect::<Vec<_>>();
        state.work_menu(&rows).unwrap();
        assert_eq!(state.menu().rows.iter().map(|row| row.annotation.as_str()).collect::<Vec<_>>(), ["ready", "blocked", "held", "closed"]);
        assert!(state.menu().rows[2].description.contains("3m held / 5m ETA"));
        state.query_menu("Archer").unwrap();
        assert_eq!(state.menu().rows.len(), 4);
        state.move_menu(-1).unwrap();
        state.accept_menu().unwrap();
        let effect = state.host_effect().unwrap();
        assert_eq!((effect.action(), effect.payload()), ("work-show", "smashcraft#247"));
    }
}

pub fn project(state: &mut NorthState, view: &ResultView) -> NorthResult<()> {
    match view {
        ResultView::List(rows) => state.work_menu(rows),
        ResultView::Document { title, text } => {
            let lines = text.lines().enumerate().map(|(i, line)| (i.to_string(), line.to_owned())).collect::<Vec<_>>();
            let rows = lines.iter().map(|(key, line)| (key.as_str(), line.as_str(), "", "")).collect::<Vec<_>>();
            state.open_settings_menu("document", title, &rows)
        }
    }
}

pub async fn run(arguments: &[String]) -> NorthResult<()> {
    if arguments.first().is_some_and(|arg| matches!(arg.as_str(), "help" | "--help" | "-h")) {
        println!("{HELP}"); return Ok(());
    }
    let mut state = NorthState::open()?;
    state.execute_command(&format!("/work {}", arguments.join(" ")))?;
    let effect = state.host_effect().ok_or_else(|| NorthError::Usage(HELP.into()))?;
    let view = execute(effect.action(), effect.payload()).await?;
    project(&mut state, &view)?;
    match view {
        ResultView::List(_) => {
            println!("Item\tTitle\tStatus\tHolder / time / ETA");
            for row in &state.menu().rows { println!("{}\t{}\t{}\t{}", row.key, row.label, row.annotation, row.description); }
        }
        ResultView::Document { title, text } => println!("{title}\n\n{text}"),
    }
    Ok(())
}
