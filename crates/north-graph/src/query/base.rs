use std::rc::Rc;

use rusqlite::{Connection, params_from_iter};

use super::ast::View;
use super::budget::Budget;
use super::check::BaseRelation;
use super::relation::Row;
use crate::error::{QueryError, Result};
use crate::store::{ActionKind, Terms};
use crate::term::Term;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    from: i64,
    to: i64,
    assert: i64,
}

impl Window {
    pub fn resolve(connection: &Connection, terms: &mut Terms<'_>, view: View) -> Result<Option<Self>> {
        let (from, to) = match view {
            View::Current => return Ok(None),
            View::AsOf(to) => (0, to),
            View::Since { from, to } => (from, to),
        };
        let head: i64 = connection.query_row("SELECT COALESCE(MAX(seq), 0) FROM occurrences", [], |row| row.get(0))?;
        if to > head {
            return Err(QueryError::InvalidSnapshot(format!("upper bound {to} is past the head sequence {head}")).into());
        }
        let assert = terms.stored_id_of(&Term::keyword(ActionKind::Assert.keyword()))?.unwrap_or(0);
        Ok(Some(Self { from, to, assert }))
    }

    fn holds(&self, occurrence: &str) -> String {
        let Self { from, to, assert } = self;
        format!(
            "{occurrence}.action = {assert} AND {occurrence}.seq BETWEEN {from} AND {to} AND NOT EXISTS (SELECT 1 FROM withdrawals {occurrence}_w JOIN occurrences {occurrence}_r ON {occurrence}_r.coord = {occurrence}_w.retraction WHERE {occurrence}_w.assertion = {occurrence}.coord AND {occurrence}_r.seq <= {to})"
        )
    }
}

pub struct Fragment {
    pub columns: Vec<String>,
    pub from: String,
    pub conditions: Vec<String>,
}

pub fn fragment(relation: BaseRelation, window: Option<&Window>, alias: &str) -> Fragment {
    let columns = |names: &[&str]| names.iter().map(|name| format!("{alias}{name}")).collect::<Vec<_>>();
    let (columns, from, conditions) = match (relation, window) {
        (BaseRelation::Triple, None) => (columns(&[".t1", ".t2", ".t3"]), format!("live_props {alias}"), Vec::new()),
        (BaseRelation::Triple, Some(window)) => {
            let holds = window.holds(&format!("{alias}_o"));
            (columns(&[".t1", ".t2", ".t3"]), format!("quoted {alias}"), vec![format!("EXISTS (SELECT 1 FROM occurrences {alias}_o WHERE {alias}_o.prop = {alias}.id AND {holds})")])
        }
        (BaseRelation::Occurrence, window) => {
            let bounds = window.map(|Window { from, to, .. }| format!("{alias}.seq BETWEEN {from} AND {to}"));
            (columns(&[".coord", ".action", ".prop"]), format!("occurrences {alias}"), bounds.into_iter().collect())
        }
        (BaseRelation::Withdrawal, None) => (columns(&[".retraction", ".assertion"]), format!("withdrawals {alias}"), Vec::new()),
        (BaseRelation::Withdrawal, Some(Window { from, to, .. })) => (
            columns(&[".retraction", ".assertion"]),
            format!("withdrawals {alias} JOIN occurrences {alias}_r ON {alias}_r.coord = {alias}.retraction"),
            vec![format!("{alias}_r.seq BETWEEN {from} AND {to}")],
        ),
        (BaseRelation::Assertion, None) => (
            columns(&[".writer", "_p.t1", "_p.t2", "_p.t3"]),
            format!("occurrences {alias} JOIN live_props {alias}_p ON {alias}_p.prop = {alias}.prop"),
            vec![format!("{alias}.live = 1")],
        ),
        (BaseRelation::Assertion, Some(window)) => (
            columns(&[".writer", "_q.t1", "_q.t2", "_q.t3"]),
            format!("occurrences {alias} JOIN quoted {alias}_q ON {alias}_q.id = {alias}.prop"),
            vec![window.holds(alias)],
        ),
        (BaseRelation::Quoted, _) => (columns(&[".id", ".t1", ".t2", ".t3"]), format!("quoted {alias}"), Vec::new()),
    };
    Fragment { columns, from, conditions }
}

pub fn fetch(connection: &Connection, relation: BaseRelation, window: Option<&Window>, key: &[Option<i64>], budget: &Budget) -> Result<Vec<Row>> {
    if key.iter().flatten().any(|id| *id <= 0) {
        return Ok(Vec::new());
    }
    let Fragment { columns, from, mut conditions } = fragment(relation, window, "b");
    conditions.extend(columns.iter().zip(key).filter(|(_, value)| value.is_some()).map(|(column, _)| format!("{column} = ?")));
    let mut sql = format!("SELECT {} FROM {from}", columns.join(", "));
    if !conditions.is_empty() {
        sql.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
    }
    let mut statement = connection.prepare_cached(&sql)?;
    let mut rows = statement.query(params_from_iter(key.iter().flatten()))?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        budget.tick()?;
        let values: Vec<i64> = (0..columns.len()).map(|column| row.get(column)).collect::<rusqlite::Result<_>>()?;
        found.push(Rc::from(values));
    }
    Ok(found)
}
