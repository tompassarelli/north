use std::rc::Rc;

use rusqlite::{Connection, params_from_iter};

use super::budget::Budget;
use super::check::BaseRelation;
use super::relation::Row;
use crate::error::Result;

fn layout(relation: BaseRelation) -> (&'static str, &'static [&'static str]) {
    match relation {
        BaseRelation::Triple => ("live_props", &["t1", "t2", "t3"]),
        BaseRelation::Occurrence => ("occurrences", &["coord", "action", "prop"]),
        BaseRelation::Withdrawal => ("withdrawals", &["retraction", "assertion"]),
    }
}

pub fn fetch(connection: &Connection, relation: BaseRelation, key: &[Option<i64>], budget: &Budget) -> Result<Vec<Row>> {
    if key.iter().flatten().any(|id| *id <= 0) {
        return Ok(Vec::new());
    }
    let (table, columns) = layout(relation);
    let mut sql = format!("SELECT {} FROM {table}", columns.join(", "));
    let mut separator = " WHERE";
    for (column, value) in columns.iter().zip(key) {
        if value.is_some() {
            sql.push_str(&format!("{separator} {column} = ?"));
            separator = " AND";
        }
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
