use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::Console;
use super::args::Args;
use super::db::connect;
use super::env::Env;
use super::failure::{Failure, Result};
use super::index;
use super::speaker::speaker;
use super::text::clip;
use crate::fleet::sender::{Sender, urgent_text};
use crate::fleet::store::delivered_by;

pub(super) const CLAUDE_FILE: &str = "claude.jsonl";

struct Typed {
    account: Option<String>,
    session: Option<String>,
    ts: Option<String>,
    cwd: Option<String>,
    text: String,
}

impl Typed {
    fn key(&self) -> String {
        key(self.session.as_deref(), self.ts.as_deref(), &self.text)
    }

    fn line(&self) -> String {
        json!({"provider": "claude", "account": self.account, "session": self.session, "timestamp": self.ts, "cwd": self.cwd, "text": self.text}).to_string()
    }
}

fn key(session: Option<&str>, ts: Option<&str>, text: &str) -> String {
    format!("{}\u{1f}{}\u{1f}{text}", session.unwrap_or(""), ts.unwrap_or(""))
}

fn typed(con: &Connection, from_tom: &HashSet<(String, String)>) -> Result<Vec<Typed>> {
    let mut statement = con.prepare(
        "SELECT f.account, f.session_id, m.ts, f.cwd, b.text, m.role FROM msg m JOIN files f ON f.id = m.file_id JOIN body b ON b.id = m.body \
         WHERE f.provider = 'anthropic' AND f.agent IS NULL AND m.role IN ('user', 'sent') AND m.tool IS NULL ORDER BY m.ts, m.id",
    )?;
    let rows = statement.query_map([], |row| {
        let typed = Typed { account: row.get(0)?, session: row.get(1)?, ts: row.get(2)?, cwd: row.get(3)?, text: row.get(4)? };
        Ok((typed, row.get::<_, String>(5)? == "user"))
    })?;
    let rows: Vec<(Typed, bool)> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(rows.into_iter().filter(|(row, keyed)| *keyed || row.session.as_ref().is_some_and(|session| from_tom.contains(&(session.clone(), urgent_text(&row.text).unwrap_or(&row.text).to_owned())))).map(|(row, _)| row).collect())
}

fn sent_by_tom(env: &Env) -> Result<HashSet<(String, String)>> {
    let deliveries = delivered_by(&env.ledger, &Sender::Tom).map_err(|error| Failure::Other(format!("the fleet store at {} is unreadable: {}", env.ledger.display(), error.user_message())))?;
    Ok(deliveries.into_iter().map(|(conversation, text)| (conversation, clip(&text))).filter(|(_, text)| speaker("user", text) == "user").collect())
}

fn kept(path: &Path) -> Result<HashSet<String>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(error.into()),
    };
    let field = |line: &Value, name: &str| line.get(name).and_then(Value::as_str).map(str::to_owned);
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .map(|line| key(field(&line, "session").as_deref(), field(&line, "timestamp").as_deref(), &field(&line, "text").unwrap_or_default()))
        .collect())
}

pub(super) fn corpus(env: &Env, io: &mut Console, args: &Args) -> Result<u8> {
    let busy = || Failure::Unreconciled("another index run holds the lock".to_owned());
    index::refresh(env, args.quiet, false, env.lock_wait, false)?.ok_or_else(busy)?;
    let from_tom = sent_by_tom(env)?;
    let con = connect(&env.db(), &env.state, true)?;
    let path = env.corpus.join(CLAUDE_FILE);
    let mut seen = kept(&path)?;
    let fresh: Vec<Typed> = typed(&con, &from_tom)?.into_iter().filter(|row| seen.insert(row.key())).collect();
    drop(con);
    if !fresh.is_empty() {
        fs::create_dir_all(&env.corpus)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(fresh.iter().map(|row| row.line() + "\n").collect::<String>().as_bytes())?;
        file.sync_all()?;
        index::refresh(env, true, false, env.lock_wait, false)?.ok_or_else(busy)?;
    }
    writeln!(io.out, "corpus     {} typed messages added to {} ({} kept)", fresh.len(), path.display(), seen.len())?;
    Ok(0)
}
