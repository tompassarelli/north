use rusqlite::{Connection, params};

use crate::fleet::model::first_item_issue;

const INHERIT: &str = "UPDATE runs SET task_ref = (SELECT k.task_ref FROM runs k WHERE k.session = runs.session AND k.task_ref IS NOT NULL ORDER BY k.run_id LIMIT 1) \
     WHERE task_ref IS NULL AND (?1 IS NULL OR session = ?1) AND EXISTS (SELECT 1 FROM runs k WHERE k.session = runs.session AND k.task_ref IS NOT NULL)";

pub(super) fn link_by_brief(conn: &Connection, session: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(INHERIT, [session])?;
    let mut briefed = conn.prepare(
        "SELECT session, prompt FROM runs WHERE task_ref IS NULL AND prompt LIKE '%Item:%' AND (?1 IS NULL OR session = ?1) ORDER BY run_id",
    )?;
    let mut named: Vec<(String, String)> = Vec::new();
    for row in briefed.query_map([session], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
        let (owner, prompt) = row?;
        if let Some(task_ref) = first_item_issue(&prompt).filter(|_| !named.iter().any(|(seen, _)| *seen == owner)) {
            named.push((owner, task_ref));
        }
    }
    for (owner, task_ref) in named {
        conn.execute("UPDATE runs SET task_ref = ?1 WHERE session = ?2 AND task_ref IS NULL", params![task_ref, owner])?;
    }
    Ok(())
}
