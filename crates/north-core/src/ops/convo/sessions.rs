use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use super::Console;
use super::args::Args;
use super::env::Env;
use super::failure::Result;
use super::query::{quote, session_prefix};
use super::rank::snippet;
use super::render::{Palette, stamp};
use super::text::{char_prefix, short_project};

struct Transcript {
    session_id: Option<String>,
    agent: Option<String>,
    project: Option<String>,
    title: Option<String>,
    path: String,
    first_ts: Option<String>,
    last_ts: Option<String>,
    nmsg: i64,
    id: i64,
}

fn transcripts(con: &Connection, prefix: &str) -> Result<Vec<Transcript>> {
    let mut statement = con.prepare(
        "SELECT session_id, agent, project, title, path, first_ts, last_ts, nmsg, id FROM files \
         WHERE session_id LIKE ?1 OR agent LIKE ?1 ORDER BY agent IS NOT NULL, first_ts",
    )?;
    let rows = statement.query_map([format!("{prefix}%")], |row| {
        Ok(Transcript {
            session_id: row.get(0)?,
            agent: row.get(1)?,
            project: row.get(2)?,
            title: row.get(3)?,
            path: row.get(4)?,
            first_ts: row.get(5)?,
            last_ts: row.get(6)?,
            nmsg: row.get(7)?,
            id: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn mentions(con: &Connection, prefix: &str, limit: i64) -> Vec<(Option<String>, i64, Option<String>)> {
    if prefix.chars().count() < 6 {
        return Vec::new();
    }
    let run = || -> rusqlite::Result<Vec<_>> {
        con.prepare(
            "SELECT f.session_id, COUNT(*) n, MAX(m.ts) last FROM ftx JOIN msg m ON m.id = ftx.rowid JOIN files f ON f.id = m.file_id \
             WHERE ftx MATCH ? AND COALESCE(f.session_id, '') NOT LIKE ? GROUP BY f.session_id ORDER BY last DESC LIMIT ?",
        )?
        .query_map(rusqlite::params![quote(prefix), format!("{prefix}%"), limit], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
    };
    run().unwrap_or_default()
}

fn print_transcripts(con: &Connection, io: &mut Console, rows: &[Transcript], shown: &[&Transcript], args: &Args, pal: &Palette) -> Result<()> {
    let limit = if args.limit != 0 { args.limit } else { 20 } as usize;
    let live = rows.iter().filter(|row| row.nmsg != 0).count();
    writeln!(io.out, "{}{}{}  {live} transcripts", pal.bold, rows[0].session_id.as_deref().unwrap_or("None"), pal.off)?;
    if let Some(main) = rows.iter().find(|row| row.agent.is_none() && row.nmsg != 0) {
        let ask: Option<String> = con
            .query_row(
                "SELECT b.text FROM msg m JOIN body b ON b.id = m.body WHERE m.file_id = ? AND m.role = 'user' ORDER BY m.line LIMIT 1",
                [main.id],
                |row| row.get(0),
            )
            .ok();
        if let Some(ask) = ask {
            writeln!(io.out, "  ask · {}", snippet(&ask, &[], 200))?;
        }
    }
    for row in shown.iter().take(limit) {
        let last: String = stamp(row.last_ts.as_deref()).chars().skip(11).collect();
        let span = format!("{} → {last}", stamp(row.first_ts.as_deref()));
        let agent = row.agent.as_ref().map_or(String::new(), |agent| format!("agent {}  ", char_prefix(agent, 12)));
        writeln!(
            io.out,
            "  {}{}{}  {span}  {} msgs  {agent}{}",
            pal.cyan, short_project(row.project.as_deref()), pal.off, row.nmsg, row.title.as_deref().unwrap_or("")
        )?;
        writeln!(io.out, "    {}{}{}", pal.dim, row.path, pal.off)?;
    }
    if shown.len() > limit {
        writeln!(io.out, "  {}… {} more (-n){}", pal.dim, shown.len() - limit, pal.off)?;
    }
    if rows.len() > live && !args.verbose {
        writeln!(io.out, "  {}({} files without messages; -v lists them){}", pal.dim, rows.len() - live, pal.off)?;
    }
    Ok(())
}

pub(super) fn session(con: &Connection, io: &mut Console, args: &Args, pal: &Palette) -> Result<u8> {
    let prefix = session_prefix(&args.query[0]);
    let rows = transcripts(con, &prefix)?;
    let mut sessions: Vec<String> = rows.iter().map(|row| row.session_id.clone().unwrap_or_else(|| "None".to_owned())).collect();
    sessions.sort();
    sessions.dedup();
    let limit = if args.limit != 0 { args.limit } else { 20 };
    if sessions.len() > 1 {
        writeln!(io.out, "{prefix} matches {} sessions; give more characters:", sessions.len())?;
        for session in sessions.iter().take(limit as usize) {
            writeln!(io.out, "  {session}")?;
        }
        return Ok(0);
    }
    let live: Vec<&Transcript> = rows.iter().filter(|row| row.nmsg != 0).collect();
    let shown: Vec<&Transcript> = if args.verbose { rows.iter().collect() } else { live };
    if !shown.is_empty() {
        print_transcripts(con, io, &rows, &shown, args, pal)?;
    }
    let mentioned = mentions(con, &prefix, limit);
    if shown.is_empty() && mentioned.is_empty() {
        writeln!(io.out, "no transcript or mention found for {prefix}")?;
        return Ok(1);
    }
    if !mentioned.is_empty() {
        writeln!(io.out, "{}mentioned in {} other session(s){}", pal.bold, mentioned.len(), pal.off)?;
        for (session, count, last) in mentioned {
            writeln!(io.out, "  {}  {}  {count} msgs", char_prefix(session.as_deref().unwrap_or("?"), 8), stamp(last.as_deref()))?;
        }
    }
    Ok(0)
}

pub(super) fn status(con: &Connection, env: &Env, io: &mut Console) -> Result<u8> {
    let (count, bytes, disk, messages, packed): (i64, Option<i64>, Option<i64>, Option<i64>, Option<i64>) = con.query_row(
        "SELECT COUNT(*) n, SUM(size) b, SUM(dsize) d, SUM(nmsg) m, SUM(zst) z FROM files",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?;
    let texts: i64 = con.query_row("SELECT COUNT(*) FROM body", [], |row| row.get(0))?;
    let db = env.db();
    let size: u64 = ["", "-wal"].iter().filter_map(|suffix| std::fs::metadata(format!("{}{suffix}", db.display())).ok()).map(|meta| meta.len()).sum();
    let meta: std::collections::HashMap<String, String> = con
        .prepare("SELECT k, v FROM meta")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let gib = |bytes: i64| bytes as f64 / (1u64 << 30) as f64;
    writeln!(io.out, "index    {}  {:.1} MiB", db.display(), size as f64 / (1u64 << 20) as f64)?;
    writeln!(io.out, "corpus   {:.1} GiB in {count} transcripts, {:.1} GiB on disk ({} compressed)", gib(bytes.unwrap_or(0)), gib(disk.unwrap_or(0)), packed.unwrap_or(0))?;
    writeln!(io.out, "messages {} ({texts} distinct texts)", messages.unwrap_or(0))?;
    if let Some(last) = meta.get("last_index").and_then(|value| value.parse::<f64>().ok()) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64());
        writeln!(io.out, "updated  {:.0}s ago", now - last)?;
    }
    if let Some(roots) = meta.get("last_roots").and_then(|value| serde_json::from_str::<Vec<serde_json::Value>>(value).ok()) {
        writeln!(io.out, "roots    {} reconciled", roots.len())?;
    }
    writeln!(io.out, "top projects")?;
    let mut statement = con.prepare("SELECT project, SUM(nmsg) m FROM files GROUP BY project ORDER BY m DESC LIMIT 8")?;
    for row in statement.query_map([], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<i64>>(1)?)))? {
        let (project, count) = row?;
        writeln!(io.out, "  {:<28} {}", short_project(project.as_deref()), count.unwrap_or(0))?;
    }
    Ok(0)
}
