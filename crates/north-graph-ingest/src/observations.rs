use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::process::Command;

use north_graph::{Action, Atom, Graph, Term, Triple, parse_query};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::facts::{Facts, text};
use crate::places::Places;

const LANDER: &str = "lander";
const GIT: &str = "git";
const LANDED_ON: &str = "landed-on";
const ANCESTOR_OF: &str = "ancestor-of";
const MAIN: &str = "main";
const ORIGIN_MAIN: &str = "origin/main";

#[derive(Debug, Default)]
pub struct Appended {
    pub asserted: usize,
    pub unchanged: usize,
    pub notes: Vec<String>,
}

pub fn append(graph: &mut Graph, places: &Places) -> Result<Appended, String> {
    let mut appended = Appended::default();
    carried_checks(graph, places, &mut appended)?;
    let landed = fresh(&landings(places, &mut appended.notes), &subjects(graph, LANDER, LANDED_ON)?, &mut appended);
    let known = subjects(graph, GIT, ANCESTOR_OF)?;
    let mut unseen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (repo, sha) in claimed(graph)? {
        if known.contains(&sha) {
            appended.unchanged += 1;
        } else {
            unseen.entry(repo).or_default().insert(sha);
        }
    }
    let mut ancestors = Vec::new();
    for (repo, shas) in &unseen {
        match on_main(places, repo) {
            Ok(main) => ancestors.extend(shas.iter().filter(|sha| main.contains(*sha)).cloned()),
            Err(why) => appended.notes.push(format!("no ancestry of {} claimed commits of {repo}: {why}", shas.len())),
        }
    }
    let told = |shas: &[String], predicate: &str, object: &str| -> Vec<Action> { shas.iter().map(|sha| Action::assert(Triple::new(text(sha), Term::keyword(predicate), text(object)))).collect() };
    let (landed, ancestors) = (told(&landed, LANDED_ON, MAIN), told(&ancestors, ANCESTOR_OF, ORIGIN_MAIN));
    appended.asserted += landed.len() + ancestors.len();
    graph.apply_batches(&[(LANDER, &landed), (GIT, &ancestors)]).map_err(|why| why.to_string())?;
    Ok(appended)
}

fn fresh(found: &BTreeSet<String>, known: &HashSet<String>, appended: &mut Appended) -> Vec<String> {
    let new: Vec<String> = found.iter().filter(|sha| !known.contains(*sha)).cloned().collect();
    appended.unchanged += found.len() - new.len();
    new
}

fn landings(places: &Places, notes: &mut Vec<String>) -> BTreeSet<String> {
    let mut landed: BTreeSet<String> = north_fleet::fleet::landings::pushes(&places.pushes).into_iter().map(|push| push.landed).collect();
    match ledger_landings(places) {
        Ok(found) => landed.extend(found),
        Err(why) => notes.push(format!("no landings from {}: {why}", places.ledger.display())),
    }
    match threads_landings(places) {
        Ok(found) => landed.extend(found),
        Err(why) => notes.push(format!("no landings from {}: {why}", places.threads.display())),
    }
    landed.retain(|sha| !sha.is_empty());
    landed.into_iter().map(|sha| sha.to_ascii_lowercase()).collect()
}

fn ledger_landings(places: &Places) -> rusqlite::Result<Vec<String>> {
    if !places.ledger.is_file() {
        return Ok(Vec::new());
    }
    let connection = Connection::open_with_flags(&places.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare("SELECT landed_sha FROM tasks WHERE outcome = 'landed' AND landed_sha IS NOT NULL")?;
    statement.query_map([], |row| row.get::<_, String>(0))?.collect()
}

fn threads_landings(places: &Places) -> rusqlite::Result<Vec<String>> {
    let Some(connection) = threads(places, "landings")? else { return Ok(Vec::new()) };
    let mut statement = connection.prepare("SELECT landed_sha FROM landings WHERE verdict = 'landed' AND landed_sha IS NOT NULL")?;
    statement.query_map([], |row| row.get::<_, String>(0))?.collect()
}

fn carried_checks(graph: &mut Graph, places: &Places, appended: &mut Appended) -> Result<(), String> {
    let read = || -> rusqlite::Result<Facts> {
        let mut facts = Facts::new(&[]);
        let Some(connection) = threads(places, "done_checks")? else { return Ok(facts) };
        let mut statement = connection.prepare("SELECT task_ref, head_sha, command, exit FROM done_checks")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)))?;
        for row in rows {
            let (task, sha, command, exit) = row?;
            crate::ledger::check(&mut facts, &task, &sha, command, exit);
        }
        Ok(facts)
    };
    let facts = match read() {
        Ok(facts) => facts,
        Err(why) => {
            appended.notes.push(format!("no carried checks from {}: {why}", places.threads.display()));
            return Ok(());
        }
    };
    let synced = graph.sync(&facts.finish()).map_err(|why| why.to_string())?;
    appended.asserted += synced.asserted;
    appended.unchanged += synced.unchanged;
    Ok(())
}

fn threads(places: &Places, table: &str) -> rusqlite::Result<Option<Connection>> {
    if !places.threads.is_file() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(&places.threads, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let found = connection.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |_| Ok(())).optional()?;
    Ok(found.map(|()| connection))
}

fn on_main(places: &Places, repo: &str) -> Result<HashSet<String>, String> {
    let checkout = places.code.join(repo).join("main");
    let output = Command::new("git").arg("-C").arg(&checkout).args(["rev-list", ORIGIN_MAIN]).output().map_err(|why| format!("git in {}: {why}", checkout.display()))?;
    if !output.status.success() {
        return Err(format!("git rev-list {ORIGIN_MAIN} in {}: {}", checkout.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect())
}

fn strings(graph: &Graph, query: &serde_json::Value) -> Result<Vec<Vec<String>>, String> {
    let query = parse_query(&query.to_string()).map_err(|why| why.to_string())?;
    let answer = graph.ask(&query).map_err(|why| why.to_string())?;
    Ok(answer.rows.into_iter().filter_map(|row| row.iter().map(string).collect()).collect())
}

fn subjects(graph: &Graph, writer: &str, predicate: &str) -> Result<HashSet<String>, String> {
    let rule = serde_json::json!({"head": {"rel": "seen", "args": [{"var": "s"}]}, "body": [
        {"rel": "assertion", "args": [writer, {"var": "s"}, ["keyword", predicate], {"var": "o"}]}]});
    Ok(strings(graph, &serde_json::json!({"find": "seen", "rules": [rule]}))?.into_iter().flatten().collect())
}

fn claimed(graph: &Graph) -> Result<BTreeSet<(String, String)>, String> {
    let rule = serde_json::json!({"head": {"rel": "claimed", "args": [{"var": "box"}, {"var": "sha"}]}, "body": [
        {"rel": "assertion", "args": [{"var": "who"}, {"var": "speaker"}, ["keyword", "claims"], {"var": "q"}]},
        {"pred": "eq", "args": [{"var": "who"}, {"var": "speaker"}]},
        {"rel": "quoted", "args": [{"var": "q"}, {"var": "box"}, ["keyword", "done-at"], {"var": "sha"}]}]});
    let rows = strings(graph, &serde_json::json!({"find": "claimed", "rules": [rule]}))?;
    Ok(rows.into_iter().filter_map(|row| Some((repo(row.first()?)?.to_owned(), row.get(1)?.clone()))).collect())
}

fn repo(node: &str) -> Option<&str> {
    node.strip_prefix("box:")?.split_once('#').map(|(repo, _)| repo)
}

fn string(term: &Term) -> Option<String> {
    match term {
        Term::Atom(Atom::String(value)) => Some(value.clone()),
        _ => None,
    }
}
