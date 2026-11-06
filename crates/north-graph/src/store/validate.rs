use rusqlite::Connection;

use super::resolve::Terms;
use super::Graph;
use crate::error::{GraphError, Result};
use crate::key::{Key, decode_key};
use crate::term::{Atom, Term};

const PROBLEM_LIMIT: usize = 20;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub terms: u64,
    pub occurrences: u64,
    pub live_occurrences: u64,
    pub live_triples: u64,
    pub problems: Vec<String>,
}

impl Report {
    pub fn is_sound(&self) -> bool {
        self.problems.is_empty()
    }

    fn problem(&mut self, found: String) {
        if self.problems.len() < PROBLEM_LIMIT {
            self.problems.push(found);
        }
    }

    fn count(&mut self, problem: &str, count: i64) {
        if count > 0 {
            self.problem(format!("{problem}: {count}"));
        }
    }
}

impl Graph {
    pub fn validate(&self) -> Result<Report> {
        let snapshot = self.connection().unchecked_transaction()?;
        let mut report = Report::default();
        let triples = check_terms(&snapshot, &mut report)?;
        check_occurrences(&snapshot, &triples, &mut report)?;
        check_withdrawals(&snapshot, &mut report)?;
        check_live_triples(&snapshot, &mut report)?;
        check_coordinates(&snapshot, &mut report)?;
        Ok(report)
    }
}

fn scalar(connection: &Connection, sql: &str) -> Result<i64> {
    Ok(connection.query_row(sql, [], |row| row.get(0))?)
}

fn check_terms(connection: &Connection, report: &mut Report) -> Result<Vec<bool>> {
    let top = usize::try_from(scalar(connection, "SELECT COALESCE(MAX(id), 0) FROM terms")?).unwrap_or(0);
    let mut triples = vec![false; top + 1];
    let mut present = vec![false; top + 1];
    let mut statement = connection.prepare("SELECT id, key FROM terms ORDER BY id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let key: Vec<u8> = row.get(1)?;
        report.terms += 1;
        let slot = usize::try_from(id).unwrap_or(0);
        present[slot] = true;
        match decode_key(&key) {
            Err(why) => report.problem(format!("term {id}: {why}")),
            Ok(Key::Atom(_)) => {}
            Ok(Key::Triple(children)) => {
                triples[slot] = true;
                let dangling = children.iter().any(|child| usize::try_from(*child).ok().is_none_or(|child| child >= slot || !present[child]));
                if dangling {
                    report.problem(format!("triple term {id} has a child that does not precede it"));
                }
            }
        }
    }
    Ok(triples)
}

fn keyword_id(connection: &Connection, name: &str) -> Result<Option<i64>> {
    Terms::new(connection).stored_id_of(&Term::keyword(name))
}

fn check_occurrences(connection: &Connection, triples: &[bool], report: &mut Report) -> Result<()> {
    let assert_id = keyword_id(connection, "assert")?;
    let retract_id = keyword_id(connection, "retract")?;
    let is_triple = |id: i64| usize::try_from(id).ok().and_then(|slot| triples.get(slot)).copied().unwrap_or(false);
    let mut statement = connection.prepare("SELECT seq, coord, action, prop, live FROM occurrences")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let (seq, coord, action, prop, live): (i64, i64, i64, i64, bool) = (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?);
        report.occurrences += 1;
        report.live_occurrences += u64::from(live);
        if !is_triple(coord) || !is_triple(prop) {
            report.problem(format!("occurrence {seq}: coordinate or proposition is not a triple term"));
        }
        if Some(action) != assert_id && Some(action) != retract_id {
            report.problem(format!("occurrence {seq}: action is neither :assert nor :retract"));
        }
        if live && Some(action) != assert_id {
            report.problem(format!("occurrence {seq}: a retraction is marked live"));
        }
    }
    Ok(())
}

fn check_withdrawals(connection: &Connection, report: &mut Report) -> Result<()> {
    let wrong_pair = scalar(
        connection,
        "SELECT COUNT(*) FROM withdrawals w
         LEFT JOIN occurrences r ON r.coord = w.retraction
         LEFT JOIN occurrences a ON a.coord = w.assertion
         WHERE r.seq IS NULL OR a.seq IS NULL OR r.prop != a.prop OR a.seq >= r.seq OR r.live != 0
            OR r.action = a.action",
    )?;
    report.count("withdrawals that do not bind a later retraction to an earlier assertion of the same proposition", wrong_pair);
    let liveness = scalar(
        connection,
        "SELECT COUNT(*) FROM occurrences o
         WHERE o.live = 1 AND EXISTS (SELECT 1 FROM withdrawals w WHERE w.assertion = o.coord)",
    )?;
    report.count("withdrawn assertions still marked live", liveness);
    let unmarked = scalar(
        connection,
        "SELECT COUNT(*) FROM withdrawals w JOIN occurrences a ON a.coord = w.assertion WHERE a.live = 1",
    )?;
    report.count("withdrawals whose assertion is still live", unmarked);
    if let Some(assert_id) = keyword_id(connection, "assert")? {
        let dead = scalar(
            connection,
            &format!(
                "SELECT COUNT(*) FROM occurrences o WHERE o.action = {assert_id} AND o.live = 0
                 AND NOT EXISTS (SELECT 1 FROM withdrawals w WHERE w.assertion = o.coord)"
            ),
        )?;
        report.count("assertions that are not live and were never withdrawn", dead);
    }
    Ok(())
}

fn check_live_triples(connection: &Connection, report: &mut Report) -> Result<()> {
    let counts = scalar(
        connection,
        "SELECT COUNT(*) FROM (SELECT prop, COUNT(*) AS c FROM occurrences WHERE live = 1 GROUP BY prop) a
         LEFT JOIN live_props l ON l.prop = a.prop WHERE l.n IS NOT a.c",
    )?;
    report.count("live propositions whose count disagrees with their live assertions", counts);
    let stale = scalar(connection, "SELECT COUNT(*) FROM live_props l WHERE NOT EXISTS (SELECT 1 FROM occurrences o WHERE o.prop = l.prop AND o.live = 1)")?;
    report.count("live propositions with no live assertion", stale);
    let mut statement = connection.prepare("SELECT l.t1, l.t2, l.t3, t.key FROM live_props l JOIN terms t ON t.id = l.prop")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        report.live_triples += 1;
        let columns = [row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?];
        let key: Vec<u8> = row.get(3)?;
        if !matches!(decode_key(&key), Ok(Key::Triple(children)) if children == columns) {
            report.problem("a live proposition's positions disagree with its term".to_string());
        }
    }
    Ok(())
}

fn check_coordinates(connection: &Connection, report: &mut Report) -> Result<()> {
    let mut terms = Terms::new(connection);
    let mut statement = connection.prepare("SELECT seq, coord FROM occurrences ORDER BY seq")?;
    let mut rows = statement.query([])?;
    let mut previous: Option<(i64, i64)> = None;
    while let Some(row) = rows.next()? {
        let (seq, coord): (i64, i64) = (row.get(0)?, row.get(1)?);
        let Some(position) = coordinate_position(&mut terms, coord)? else {
            report.problem(format!("occurrence {seq}: coordinate is not (transaction, :kernel/op-ordinal, ordinal)"));
            continue;
        };
        if previous.is_some_and(|before| before >= position) {
            report.problem(format!("occurrence {seq}: coordinate does not follow the previous occurrence"));
        }
        previous = Some(position);
    }
    Ok(())
}

fn coordinate_position(terms: &mut Terms<'_>, coordinate: i64) -> Result<Option<(i64, i64)>> {
    let term = match terms.term(coordinate) {
        Ok(term) => term,
        Err(GraphError::Corrupt(_)) => return Ok(None),
        Err(other) => return Err(other),
    };
    let position = term.as_triple().and_then(|op| match (op.t1.as_triple(), &op.t2, &op.t3) {
        (Some(transaction), Term::Atom(Atom::Keyword(op_kind)), Term::Atom(Atom::Int(ordinal))) if op_kind == "kernel/op-ordinal" => match (&transaction.t1, &transaction.t2, &transaction.t3) {
            (Term::Atom(Atom::String(_)), Term::Atom(Atom::Keyword(tx_kind)), Term::Atom(Atom::Int(sequence))) if tx_kind == "kernel/tx-sequence" => Some((*sequence, *ordinal)),
            _ => None,
        },
        _ => None,
    });
    Ok(position)
}
