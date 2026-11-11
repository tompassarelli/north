//! The checkbox lines of an issue's body as the ledger last read them, one row per box and the commit shas its line names:
//! the record a split run's settlement reads for the boxes its session ticked and the boxes left open (north#872).
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, params};

use super::claims::box_slug;
use super::runs::row::Row;
use super::verb::Failure;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS issue_boxes(
  item TEXT NOT NULL,
  slug TEXT NOT NULL,
  ticked INTEGER NOT NULL CHECK(ticked IN (0, 1)),
  sha TEXT NOT NULL,
  PRIMARY KEY(item, slug, sha)
) STRICT";

pub const KIND: &str = "issue_boxes";

static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").expect("the sha pattern compiles"));

pub fn row(item: &str, body: &str) -> String {
    format!("{}\n", serde_json::json!({"type": KIND, "item": item, "body": body}))
}

pub fn ensure(db: &Connection) -> Result<(), Failure> {
    db.execute(SCHEMA, [])?;
    Ok(())
}

struct Checkbox {
    slug: String,
    ticked: bool,
    shas: Vec<String>,
}

fn checkbox(line: &str) -> Option<Checkbox> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* "))?;
    let (ticked, text) = match rest.get(..3)? {
        "[ ]" => (false, &rest[3..]),
        "[x]" | "[X]" => (true, &rest[3..]),
        _ => return None,
    };
    let shas = if ticked { SHA.find_iter(&text.to_ascii_lowercase()).map(|found| found.as_str().to_owned()).collect() } else { Vec::new() };
    Some(Checkbox { slug: box_slug(text)?, ticked, shas })
}

/// Replaces the boxes held for the issue the row names with the ones its body now has; returns how many it wrote.
pub fn record(db: &Connection, row: &Row) -> Result<usize, Failure> {
    let field = |key: &str| row.get(key).and_then(|value| value.as_str()).map(str::to_owned).unwrap_or_default();
    let (item, body) = (field("item"), field("body"));
    if item.is_empty() {
        return Ok(0);
    }
    db.execute("DELETE FROM issue_boxes WHERE item = ?1", [&item])?;
    let mut written = 0;
    for found in body.lines().filter_map(checkbox) {
        let shas = if found.shas.is_empty() { vec![String::new()] } else { found.shas };
        for sha in shas {
            written += db.execute("INSERT OR IGNORE INTO issue_boxes(item, slug, ticked, sha) VALUES (?1, ?2, ?3, ?4)", params![item, found.slug, found.ticked, sha])?;
        }
    }
    Ok(written)
}
