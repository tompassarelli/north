use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

pub const OWNER: &str = "tompassarelli";
pub const REPOS: [&str; 3] = ["north", "makgora", "wisp"];
pub const FRESH_S: f64 = 5.0 * 60.0;
pub const OPEN_ISSUES: &str = "issues/open";
pub const ISSUES_PROBE: &str = "issues/probe";
pub const MAIN_RUNS: &str = "runs/main";
pub const MAIN_CHECKS: &str = "checks/main";

pub const OPEN_ISSUES_QUERY: &str = "query($owner: String!, $name: String!, $cursor: String) { repository(owner: $owner, name: $name) { issues(states: OPEN, first: 50, after: $cursor) { pageInfo { hasNextPage endCursor } nodes { number title body createdAt labels(first: 30) { nodes { name } } blockedBy(first: 30) { nodes { number state repository { name } } } lastEditedAt comments(last: 10) { nodes { body createdAt updatedAt } } } } } }";

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS reads(key TEXT PRIMARY KEY, body TEXT NOT NULL CHECK(json_valid(body)), etag TEXT, fetched REAL NOT NULL, synced REAL NOT NULL) STRICT;";

#[derive(Clone, Debug, PartialEq)]
pub struct Copy {
    pub body: Value,
    pub etag: Option<String>,
    pub fetched: f64,
    pub synced: f64,
}

#[derive(Clone, Debug)]
pub struct Mirror {
    path: PathBuf,
}

pub fn key(repo: &str, kind: &str) -> String {
    format!("{repo}/{kind}")
}

impl Mirror {
    pub fn local() -> Self {
        let path = std::env::var_os("NORTH_GITHUB_MIRROR").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north/github-mirror.db")
        });
        Self { path }
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self, key: &str) -> Option<Copy> {
        let connection = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
        connection.busy_timeout(std::time::Duration::from_secs(5)).ok()?;
        let row: Option<(String, Option<String>, f64, f64)> = connection
            .query_row("SELECT body, etag, fetched, synced FROM reads WHERE key = ?1", [key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .optional()
            .ok()?;
        let (body, etag, fetched, synced) = row?;
        Some(Copy { body: serde_json::from_str(&body).ok()?, etag, fetched, synced })
    }

    pub fn fresh(&self, key: &str, now: f64) -> Option<Value> {
        self.read(key).filter(|copy| now - copy.synced <= FRESH_S).map(|copy| copy.body)
    }

    pub fn write(&self, key: &str, body: &Value, etag: Option<&str>, at: f64) -> Result<(), String> {
        self.writable()?
            .execute(
                "INSERT INTO reads(key, body, etag, fetched, synced) VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(key) DO UPDATE SET body = excluded.body, etag = excluded.etag, fetched = excluded.fetched, synced = excluded.synced",
                params![key, body.to_string(), etag, at],
            )
            .map(drop)
            .map_err(|error| self.failure(error))
    }

    pub fn confirm(&self, key: &str, at: f64) -> Result<(), String> {
        self.writable()?.execute("UPDATE reads SET synced = ?2 WHERE key = ?1", params![key, at]).map(drop).map_err(|error| self.failure(error))
    }

    fn writable(&self) -> Result<Connection, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let connection = Connection::open(&self.path).map_err(|error| self.failure(error))?;
        connection.busy_timeout(std::time::Duration::from_secs(30)).map_err(|error| self.failure(error))?;
        connection.execute_batch(SCHEMA).map_err(|error| self.failure(error))?;
        Ok(connection)
    }

    fn failure(&self, error: rusqlite::Error) -> String {
        format!("github mirror {}: {error}", self.path.display())
    }
}

pub fn fetch_open_issues(repo: &str) -> Result<Vec<Value>, String> {
    let (mut found, mut cursor) = (Vec::new(), None::<String>);
    loop {
        let (query, owner, name) = (format!("query={OPEN_ISSUES_QUERY}"), format!("owner={OWNER}"), format!("name={repo}"));
        let mut args = vec!["api", "graphql", "-f", &query, "-F", &owner, "-F", &name];
        let after = cursor.as_ref().map(|cursor| format!("cursor={cursor}"));
        if let Some(after) = &after {
            args.extend(["-F", after]);
        }
        let issues = crate::platform::github::json(&args)?["data"]["repository"]["issues"].take();
        found.extend(issues["nodes"].as_array().into_iter().flatten().cloned());
        if issues["pageInfo"]["hasNextPage"] != true {
            return Ok(found);
        }
        cursor = issues["pageInfo"]["endCursor"].as_str().map(str::to_owned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_copy_is_fresh_until_five_minutes_after_its_last_sync_and_a_confirm_renews_it() {
        let root = tempfile::tempdir().unwrap();
        let mirror = Mirror::at(root.path().join("state/github-mirror.db"));
        assert_eq!(mirror.fresh("north/issues/open", 0.0), None);
        mirror.write("north/issues/open", &json!([{"number": 1}]), Some("\"e1\""), 1000.0).unwrap();
        assert_eq!(mirror.fresh("north/issues/open", 1000.0 + FRESH_S), Some(json!([{"number": 1}])));
        assert_eq!(mirror.fresh("north/issues/open", 1001.0 + FRESH_S), None);
        mirror.confirm("north/issues/open", 2000.0).unwrap();
        let copy = mirror.read("north/issues/open").unwrap();
        assert_eq!((copy.etag.as_deref(), copy.fetched, copy.synced), (Some("\"e1\""), 1000.0, 2000.0));
    }
}
