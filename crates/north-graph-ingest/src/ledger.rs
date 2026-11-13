use std::collections::BTreeMap;

use north_graph::Telling;
use north_threads::threads::claims;
use rusqlite::{Connection, OpenFlags};

use crate::boxes::{self, Item};
use crate::facts::{Facts, issue_ref, text};
use crate::issues;
use crate::places::Places;

const CHECKER: &str = "checker";
const LEDGER: &str = "ledger";
const THREADS: &str = "threads";

pub fn read(places: &Places) -> Result<Telling, String> {
    let connection = Connection::open_with_flags(&places.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|why| format!("{}: {why}", places.ledger.display()))?;
    let mut facts = Facts::new(&["holds"]);
    checks(&connection, &mut facts).map_err(|why| format!("{}: checks: {why}", places.ledger.display()))?;
    tasks(&connection, &mut facts).map_err(|why| format!("{}: tasks: {why}", places.ledger.display()))?;
    if places.threads.is_file() {
        let threads = claims::ledger::open(&places.threads, false)?;
        holds(&claims::ledger::claims(&threads)?, &issues::bodies(places), &mut facts);
    }
    Ok(facts.finish())
}

fn holds(held: &[claims::Claim], bodies: &BTreeMap<String, String>, facts: &mut Facts) {
    for claim in held {
        let Some(issue) = issue_ref(&claim.item) else { continue };
        let Some(body) = bodies.get(&issue) else { continue };
        let Some(slug) = &claim.slug else {
            facts.tell(THREADS, text(&claim.owner), "holds", text(&issue));
            continue;
        };
        let named = |item: &&Item| claims::box_slug(&item.text).is_some_and(|whole| whole.starts_with(slug.as_str()) || slug.starts_with(whole.as_str()));
        let items = boxes::parse(body);
        if let [only] = items.iter().filter(named).collect::<Vec<_>>()[..] {
            facts.tell(THREADS, text(&claim.owner), "holds", text(only.id(&issue)));
        }
    }
}

fn checks(connection: &Connection, facts: &mut Facts) -> rusqlite::Result<()> {
    let mut statement = connection.prepare("SELECT task_ref, head_sha, command, exit FROM checks WHERE head_sha IS NOT NULL AND head_sha != ''")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)))?;
    for row in rows {
        let (task, sha, command, exit) = row?;
        check(facts, &task, &sha, command, exit);
    }
    Ok(())
}

pub fn check(facts: &mut Facts, task: &str, sha: &str, command: String, exit: i64) {
    let sha = sha.to_ascii_lowercase();
    facts.tell(CHECKER, text(&sha), if exit == 0 { "check-passed" } else { "check-failed" }, text(command.as_str()));
    if let Some(issue) = issue_ref(task) {
        facts.tell(CHECKER, text(&sha), "checked-for", text(&issue));
        if exit == 0 {
            facts.tell(CHECKER, text(&issue), "passed-check", text(command));
        }
    }
}

fn tasks(connection: &Connection, facts: &mut Facts) -> rusqlite::Result<()> {
    let mut statement = connection.prepare("SELECT task_ref, sessions, landed_sha FROM tasks")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))?;
    for row in rows {
        let (task, sessions, landed) = row?;
        let Some(issue) = issue_ref(&task) else { continue };
        let sessions: Vec<String> = serde_json::from_str(&sessions).unwrap_or_default();
        for session in sessions {
            facts.tell(LEDGER, text(session), "works-on", text(&issue));
        }
        if let Some(sha) = landed.filter(|sha| !sha.is_empty()) {
            facts.tell(LEDGER, text(&issue), "landed-as", text(sha.to_ascii_lowercase()));
        }
    }
    Ok(())
}
