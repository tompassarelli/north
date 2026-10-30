//! Which issues are open and what blocks them: GitHub's open issues behind the ledger's minute-long
//! gh_cache, one GitHub issue, and the native items with the local needs added to either.
use std::cell::RefCell;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value as Json;

use crate::platform::pyjson::{self, Value};
use crate::threads::catalog::CATALOG;
use crate::threads::withheld::withheld;
use crate::threads::verb::{Context, Failure, is_native_ref, item_ref};

const CACHE_SECONDS: f64 = 60.0;

const OPEN_ISSUES: &str = "
query($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    issues(states: OPEN, first: 100, after: $cursor) {
      pageInfo { hasNextPage endCursor }
      nodes { number title labels(first: 30) { nodes { name } } comments(last: 10) { nodes { body createdAt } } blockedBy(first: 50) { nodes { number state repository { nameWithOwner } } } }
    }
  }
}";

const ONE_ISSUE: &str = "
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      number title state blockedBy(first: 50) { nodes { number state repository { nameWithOwner } } }
    }
  }
}";

#[derive(Clone)]
pub(crate) struct Blocker {
    pub reference: String,
    /// `OPEN` or `CLOSED`.
    pub state: String,
}

#[derive(Clone)]
pub(crate) struct Issue {
    pub number: u64,
    pub title: String,
    /// `OPEN` or `CLOSED`.
    pub state: String,
    pub blockers: Vec<Blocker>,
    /// Why a worker has already reported the issue cannot be staffed, from its comments and labels.
    pub withheld: Option<String>,
}

/// The ledger and the caller, read through one connection; a replica's gh_cache writes stay in a
/// memory copy, so the snapshot file is never changed.
pub(crate) struct Board<'a> {
    db: RefCell<Connection>,
    context: &'a Context<'a>,
}

impl<'a> Board<'a> {
    pub(crate) fn open(context: &'a Context<'a>) -> Result<Self, Failure> {
        let file = crate::threads::verb::open(context.database, !context.replica)?;
        let db = if context.replica {
            let mut copy = Connection::open_in_memory()?;
            rusqlite::backup::Backup::new(&file, &mut copy)?.run_to_completion(256, std::time::Duration::ZERO, None)?;
            copy
        } else {
            file
        };
        Ok(Self { db: RefCell::new(db), context })
    }

    pub(crate) fn db(&self) -> std::cell::Ref<'_, Connection> {
        self.db.borrow()
    }

    /// The repo's open issues in GitHub's order, or a namespace's open items by number.
    pub(crate) fn open_issues(&self, repo: &str) -> Result<Vec<Issue>, Failure> {
        if CATALOG.is_native(repo) {
            let items: Vec<(u64, String)> = {
                let db = self.db();
                let mut select = db.prepare("SELECT number, title FROM items WHERE ns = ?1 AND state = 'open' ORDER BY number")?;
                select.query_map([repo], |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)))?.collect::<Result<_, _>>()?
            };
            return items
                .into_iter()
                .map(|(number, title)| Ok(Issue { number, title, state: "OPEN".into(), blockers: self.local_blockers(&format!("{repo}#{number}"))?, withheld: None }))
                .collect();
        }
        if let Some(cached) = self.cached(repo)? {
            return Ok(cached);
        }
        let (mut issues, mut cursor): (Vec<Issue>, Option<String>) = (Vec::new(), None);
        loop {
            let query = format!("query={OPEN_ISSUES}");
            let (owner, name) = (format!("owner={}", CATALOG.owner), format!("name={repo}"));
            let mut args = vec!["api", "graphql", "-f", &query, "-F", &owner, "-F", &name];
            let after = cursor.as_ref().map(|cursor| format!("cursor={cursor}"));
            if let Some(after) = &after {
                args.extend(["-F", after]);
            }
            let reply = parsed(&self.context.gh(&args)?)?;
            let page = &reply["data"]["repository"]["issues"];
            let nodes = page["nodes"].as_array().ok_or_else(|| unreadable("open issues"))?;
            for node in nodes {
                let issue = issue_of(node, "OPEN", self.context.now.unix_seconds_f64())?;
                match issues.iter_mut().find(|known| known.number == issue.number) {
                    Some(known) => *known = issue,
                    None => issues.push(issue),
                }
            }
            if page["pageInfo"]["hasNextPage"].as_bool() != Some(true) {
                break;
            }
            cursor = Some(page["pageInfo"]["endCursor"].as_str().ok_or_else(|| unreadable("open issues"))?.to_owned());
        }
        let fetched = self.context.now.unix_seconds_f64();
        self.db().execute("INSERT OR REPLACE INTO gh_cache(repo, fetched, issues) VALUES (?1, ?2, ?3)", params![repo, fetched, pyjson::dumps(&cache_value(&issues))])?;
        Ok(issues)
    }

    /// One issue or native item, open or closed, with its blockers.
    pub(crate) fn one_issue(&self, repo: &str, number: u64) -> Result<Issue, Failure> {
        let item = format!("{repo}#{number}");
        if CATALOG.is_native(repo) {
            let (title, state) = self.native_item(repo, number)?;
            return Ok(Issue { number, title, state: state.to_uppercase(), blockers: self.local_blockers(&item)?, withheld: None });
        }
        let mut issue = match self.cached(repo)?.and_then(|issues| issues.into_iter().find(|issue| issue.number == number)) {
            Some(cached) => cached,
            None => {
                let (owner, name, wanted) = (format!("owner={}", CATALOG.owner), format!("name={repo}"), format!("number={number}"));
                let query = format!("query={ONE_ISSUE}");
                let reply = parsed(&self.context.gh(&["api", "graphql", "-f", &query, "-F", &owner, "-F", &name, "-F", &wanted])?)?;
                let node = &reply["data"]["repository"]["issue"];
                if !node.is_object() {
                    return Err(Failure::Refused(format!("no issue {item}")));
                }
                issue_of(node, node["state"].as_str().unwrap_or_default(), self.context.now.unix_seconds_f64())?
            }
        };
        issue.blockers.extend(self.local_blockers(&item)?);
        Ok(issue)
    }

    /// A native item's title and state, refused when it does not exist.
    pub(crate) fn native_item(&self, ns: &str, number: u64) -> Result<(String, String), Failure> {
        let found = self.db().query_row("SELECT title, state FROM items WHERE ns = ?1 AND number = ?2", params![ns, number as i64], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        found.ok_or_else(|| Failure::Refused(format!("no item {ns}#{number}")))
    }

    /// The ledger's needs of an item, each open or closed; a native one this caller may not read
    /// is named only as a native item.
    pub(crate) fn local_blockers(&self, item: &str) -> Result<Vec<Blocker>, Failure> {
        let needs: Vec<String> = {
            let db = self.db();
            let mut select = db.prepare("SELECT needs FROM item_needs WHERE item = ?1 ORDER BY needs")?;
            select.query_map([item], |row| row.get(0))?.collect::<Result<_, _>>()?
        };
        let mut blockers = Vec::new();
        for need in needs {
            let hidden = self.context.no_native.is_some() && is_native_ref(&need);
            let (repo, number) = if hidden {
                let (ns, number) = need.split_once('#').unwrap_or_default();
                (ns.to_owned(), number.parse().unwrap_or_default())
            } else {
                item_ref(&need, self.context)?
            };
            let state = if CATALOG.is_native(&repo) {
                let state: Option<String> = self.db().query_row("SELECT state FROM items WHERE ns = ?1 AND number = ?2", params![repo, number as i64], |row| row.get(0)).optional()?;
                state.map_or_else(|| "CLOSED".to_owned(), |state| state.to_uppercase())
            } else if self.open_issues(&repo)?.iter().any(|issue| issue.number == number) {
                "OPEN".to_owned()
            } else {
                "CLOSED".to_owned()
            };
            blockers.push(Blocker { reference: if hidden { "a native item".into() } else { need }, state });
        }
        Ok(blockers)
    }

    fn cached(&self, repo: &str) -> Result<Option<Vec<Issue>>, Failure> {
        let row: Option<(f64, String)> = self.db().query_row("SELECT fetched, issues FROM gh_cache WHERE repo = ?1", [repo], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let Some((_, issues)) = row.filter(|(fetched, _)| self.context.now.unix_seconds_f64() - fetched < CACHE_SECONDS) else { return Ok(None) };
        let cached = pyjson::loads(issues.as_bytes()).map_err(|_| unreadable("gh_cache"))?;
        let entries = cached.as_object().ok_or_else(|| unreadable("gh_cache"))?;
        if entries.iter().any(|(_, issue)| issue.get("withheld").is_none()) {
            return Ok(None);
        }
        entries
            .iter()
            .map(|(number, issue)| {
                let number = number.as_str().and_then(|number| number.parse().ok()).ok_or_else(|| unreadable("gh_cache"))?;
                let blockers = issue["blockers"].as_array().unwrap_or_default().iter().map(|blocker| Blocker { reference: text(&blocker["ref"]), state: text(&blocker["state"]) }).collect();
                let withheld = issue["withheld"].as_string().map(|reason| reason.lossy().into_owned());
                Ok(Issue { number, title: text(&issue["title"]), state: "OPEN".into(), blockers, withheld })
            })
            .collect::<Result<_, Failure>>()
            .map(Some)
    }
}

fn text(value: &Value) -> String {
    value.as_string().map(|text| text.lossy().into_owned()).unwrap_or_default()
}

fn unreadable(what: &str) -> Failure {
    Failure::Refused(format!("gh api graphql: unreadable {what}"))
}

fn parsed(reply: &str) -> Result<Json, Failure> {
    serde_json::from_str(reply).map_err(|_| unreadable("reply"))
}

fn issue_of(node: &Json, state: &str, now: f64) -> Result<Issue, Failure> {
    let number = node["number"].as_u64().ok_or_else(|| unreadable("issue"))?;
    let blockers = node["blockedBy"]["nodes"]
        .as_array()
        .ok_or_else(|| unreadable("issue"))?
        .iter()
        .map(|blocker| {
            let full = blocker["repository"]["nameWithOwner"].as_str().unwrap_or_default();
            let repo = full.strip_prefix(&format!("{}/", CATALOG.owner)).unwrap_or(full);
            Blocker { reference: format!("{repo}#{}", blocker["number"]), state: blocker["state"].as_str().unwrap_or_default().to_owned() }
        })
        .collect();
    let labels: Vec<&str> = node["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str()).collect();
    let withheld = withheld(&labels, node["comments"]["nodes"].as_array().map_or(&[][..], Vec::as_slice), now);
    Ok(Issue { number, title: node["title"].as_str().unwrap_or_default().to_owned(), state: state.to_owned(), blockers, withheld })
}

/// The cache row bin/threads also reads: `{"N": {"title": ..., "blockers": [{"ref": ..., "state": ...}]}}`.
fn cache_value(issues: &[Issue]) -> Value {
    Value::Object(
        issues
            .iter()
            .map(|issue| {
                let blockers = issue.blockers.iter().map(|blocker| Value::object([("ref", blocker.reference.as_str().into()), ("state", blocker.state.as_str().into())])).collect();
                (issue.number.to_string().as_str().into(), Value::object([("title", issue.title.as_str().into()), ("blockers", Value::Array(blockers)), ("withheld", issue.withheld.as_deref().map_or(Value::Null, Value::from))]))
            })
            .collect(),
    )
}
