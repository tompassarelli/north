use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use super::Console;
use super::args::Args;
use super::env::Env;
use super::failure::Result;
use super::query::{Filters, Hit, Info, RELAX_BELOW, gather, plan_query, recent, session_info};
use super::rank::{Group, rank_messages, snippet};
use super::render::{Palette, Row, SHOW_CAP, json_list, render_messages, render_sessions, row_json, stamp};
use super::text::{TEXT_CAP, char_count, char_prefix, norm, repr, short_project};

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn infos_for<'a>(con: &Connection, sessions: impl Iterator<Item = &'a str>) -> Result<HashMap<String, Info>> {
    let keys: Vec<&str> = sessions.collect();
    Ok(session_info(con, &keys)?)
}

fn emit_messages(con: &Connection, io: &mut Console, hits: &[Hit], terms: &[String], args: &Args, pal: &Palette, partial: bool) -> Result<u8> {
    if hits.is_empty() {
        writeln!(io.out, "no messages match these filters")?;
        return Ok(1);
    }
    let infos = infos_for(con, hits.iter().map(|hit| hit.session.as_str()))?;
    if args.json {
        let rows: Vec<String> = hits
            .iter()
            .map(|hit| row_json(&Row { hit, info: infos.get(&hit.session), terms, hits: None, full: args.full, partial: Some(partial) }))
            .collect();
        writeln!(io.out, "{}", json_list(&rows))?;
    } else {
        writeln!(io.out, "{}", render_messages(hits, &infos, terms, pal, args.full, partial).join("\n"))?;
    }
    Ok(0)
}

struct Found {
    hits: Vec<Hit>,
    groups: Vec<Group>,
    partial: bool,
}

fn search_hits(con: &Connection, filters: &Filters, plan: &super::query::Plan, mine: &HashSet<String>) -> Result<Found> {
    let literal = plan.literal.as_deref().map(norm);
    let (mut hits, mut groups) = (Vec::new(), Vec::new());
    for expr in &plan.matches {
        if let Ok(found) = gather(con, expr, filters, &plan.terms, mine, literal.as_deref(), false) {
            (hits, groups) = found;
            break;
        }
    }
    let mut partial = false;
    if let (true, Some(expr)) = (groups.len() < RELAX_BELOW, &plan.partial) {
        let known: HashSet<String> = groups.iter().map(|group| group.session.clone()).collect();
        let (loose, extra) = gather(con, expr, filters, &plan.terms, mine, None, true)?;
        groups.extend(extra.into_iter().filter(|group| !known.contains(&group.session)).map(|group| Group { partial: true, ..group }));
        groups.sort_by_key(|group| mine.contains(&group.session));
        if hits.is_empty() {
            (hits, partial) = (loose, true);
        }
    }
    Ok(Found { hits, groups, partial })
}

pub(super) fn search(con: &Connection, env: &Env, io: &mut Console, args: &Args, pal: &Palette) -> Result<u8> {
    let filters = match Filters::new(args, unix_now()) {
        Ok(filters) => filters,
        Err(message) => return Ok(usage_failure(io, &message)),
    };
    let mine: HashSet<String> = env.self_ids.iter().cloned().collect();
    let text = args.query.join(" ");
    if text.trim().is_empty() {
        let hits = recent(con, &filters, if args.limit != 0 { args.limit } else { 20 })?;
        return emit_messages(con, io, &hits, &[], args, pal, false);
    }
    let Some(plan) = plan_query(&text, args.exact, args.all_terms) else {
        writeln!(io.err, "convo: the query has no searchable words")?;
        return Ok(2);
    };
    let Found { hits, mut groups, partial } = search_hits(con, &filters, &plan, &mine)?;
    let limit = if args.limit != 0 { args.limit } else if args.messages { 20 } else { 10 } as usize;
    if groups.is_empty() {
        writeln!(io.out, "no matches for {}{}", repr(&text), if filters.clauses.is_empty() { "" } else { " with these filters" })?;
        return Ok(1);
    }
    if args.messages {
        let mut ranked = rank_messages(&hits, &mine, filters.keep_calls, &plan.terms);
        ranked.truncate(limit);
        return emit_messages(con, io, &ranked, &plan.terms, args, pal, partial);
    }
    groups.truncate(limit);
    let infos = infos_for(con, groups.iter().map(|group| group.session.as_str()))?;
    if args.json {
        let rows: Vec<String> = groups
            .iter()
            .map(|group| row_json(&Row { hit: &group.best, info: infos.get(&group.session), terms: &plan.terms, hits: Some(group.hits), full: args.full, partial: Some(group.partial) }))
            .collect();
        writeln!(io.out, "{}", json_list(&rows))?;
    } else {
        writeln!(io.out, "{}", render_sessions(&groups, &infos, &plan.terms, pal, args.full).join("\n"))?;
    }
    Ok(0)
}

pub(super) fn usage_failure(io: &mut Console, message: &str) -> u8 {
    let _ = writeln!(io.err, "convo: {message}");
    2
}

struct Shown {
    id: i64,
    file_id: i64,
    line: i64,
    ts: Option<String>,
    role: String,
    tool: Option<String>,
    text: String,
    session_id: Option<String>,
    agent: Option<String>,
    path: String,
    project: Option<String>,
    title: Option<String>,
    provider: Option<String>,
}

fn load_message(con: &Connection, id: i64) -> Result<Option<Shown>> {
    Ok(con
        .query_row(
            "SELECT m.id, m.file_id, m.line, m.ts, m.role, m.tool, b.text, f.session_id, f.agent, f.path, f.project, f.title, f.provider \
             FROM msg m JOIN body b ON b.id = m.body JOIN files f ON f.id = m.file_id WHERE m.id = ?",
            [id],
            |row| {
                Ok(Shown {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    line: row.get(2)?,
                    ts: row.get(3)?,
                    role: row.get(4)?,
                    tool: row.get(5)?,
                    text: row.get(6)?,
                    session_id: row.get(7)?,
                    agent: row.get(8)?,
                    path: row.get(9)?,
                    project: row.get(10)?,
                    title: row.get(11)?,
                    provider: row.get(12)?,
                })
            },
        )
        .optional()?)
}

type Neighbour = (i64, i64, Option<String>, String, Option<String>, String);

fn show_context(con: &Connection, io: &mut Console, shown: &Shown, count: i64, pal: &Palette) -> Result<()> {
    let sql = "SELECT * FROM (SELECT m.id, m.line, m.ts, m.role, m.tool, b.text FROM msg m JOIN body b ON b.id = m.body \
               WHERE m.file_id = ?1 AND (m.line, m.id) < (?2, ?3) ORDER BY m.line DESC, m.id DESC LIMIT ?4) UNION ALL \
               SELECT * FROM (SELECT m.id, m.line, m.ts, m.role, m.tool, b.text FROM msg m JOIN body b ON b.id = m.body \
               WHERE m.file_id = ?1 AND (m.line, m.id) > (?2, ?3) ORDER BY m.line, m.id LIMIT ?4)";
    let mut near: Vec<Neighbour> = con
        .prepare(sql)?
        .query_map(params![shown.file_id, shown.line, shown.id, count], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    near.sort_by_key(|row| (row.1, row.0));
    writeln!(io.out, "{}context{}", pal.dim, pal.off)?;
    for (id, line, ts, role, tool, text) in near {
        let mark = if (line, id) < (shown.line, shown.id) { "↑" } else { "↓" };
        let label = tool.map_or(role.clone(), |tool| format!("{role}:{tool}"));
        let clock: String = stamp(ts.as_deref()).chars().skip(11).collect();
        writeln!(io.out, "  {mark} [{id}] {clock}  {}{label}{} · {}", pal.yellow, pal.off, snippet(&text, &[], 150))?;
    }
    Ok(())
}

fn print_message(con: &Connection, io: &mut Console, shown: &Shown, args: &Args, pal: &Palette) -> Result<()> {
    let cap = if args.full { TEXT_CAP } else { SHOW_CAP };
    let text = char_prefix(&shown.text, cap);
    let agent = shown.agent.as_ref().map_or(String::new(), |agent| format!(" · agent {agent}"));
    writeln!(
        io.out,
        "{}{}{}{agent} · {}{}{} · {} · {}",
        pal.bold, shown.session_id.as_deref().unwrap_or("None"), pal.off, pal.cyan, short_project(shown.project.as_deref()), pal.off,
        shown.title.as_deref().unwrap_or(""), super::query::provider_name(shown.provider.clone())
    )?;
    let label = shown.tool.as_ref().map_or(shown.role.clone(), |tool| format!("{}:{tool}", shown.role));
    writeln!(io.out, "{}  {}{label}{}  [{}]  {}{}:{}{}", stamp(shown.ts.as_deref()), pal.yellow, pal.off, shown.id, pal.dim, shown.path, shown.line, pal.off)?;
    writeln!(io.out, "{text}")?;
    let more = char_count(&shown.text) - char_count(text);
    if more > 0 {
        writeln!(io.out, "{}… {more} more chars (--full){}", pal.dim, pal.off)?;
    }
    if args.context != 0 {
        show_context(con, io, shown, args.context, pal)?;
    }
    Ok(())
}

pub(super) fn show(con: &Connection, io: &mut Console, args: &Args, pal: &Palette) -> Result<u8> {
    let (mut code, mut rows) = (0, Vec::new());
    for reference in &args.query {
        let digits: String = reference.chars().filter(char::is_ascii_digit).collect();
        let id: i64 = digits.parse().unwrap_or(0);
        let Some(shown) = load_message(con, id)? else {
            writeln!(io.err, "convo: no message {reference}")?;
            code = 1;
            continue;
        };
        if args.json {
            rows.push(shown_json(&shown));
        } else {
            print_message(con, io, &shown, args, pal)?;
        }
    }
    if args.json {
        writeln!(io.out, "{}", json_list(&rows))?;
    }
    Ok(code)
}

fn shown_json(shown: &Shown) -> String {
    let hit = Hit {
        id: shown.id,
        line: shown.line,
        ts: shown.ts.clone(),
        role: shown.role.clone(),
        tool: shown.tool.clone(),
        bm25: 0.0,
        session: shown.session_id.clone().unwrap_or_else(|| shown.path.clone()),
        agent: shown.agent.clone(),
        path: shown.path.clone(),
        project: shown.project.clone(),
        text: shown.text.clone(),
        w: 0.0,
    };
    let info = Info { title: shown.title.clone(), project: shown.project.clone(), provider: super::query::provider_name(shown.provider.clone()) };
    row_json(&Row { hit: &hit, info: Some(&info), terms: &[], hits: None, full: true, partial: None })
}



