use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use super::ast::{AggregateFind, AggregateOp, Find, Order, Query};
use super::ops;
use super::relation::{DerivedRelation, Row};
use crate::error::{QueryError, Result};
use crate::store::Terms;
use crate::term::{Term, numeric_cmp};

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub columns: usize,
    pub rows: Vec<Vec<Term>>,
    pub steps: u64,
}

type TermRow = Vec<Rc<Term>>;

pub fn shape(query: &Query, arity: usize, relation: &DerivedRelation, terms: &mut Terms<'_>) -> Result<(usize, Vec<Vec<Term>>)> {
    let (columns, mut rows) = match &query.find {
        Find::Relation(_) => {
            let rows = relation.rows().iter().map(|row| decode(row, terms)).collect::<Result<Vec<_>>>()?;
            (arity, rows)
        }
        Find::Aggregate(find) => (find.group.len() + find.aggregates.len(), aggregate(find, relation, terms)?),
    };
    rows.sort_by(|left, right| ordering(&query.order_by, left, right));
    if let Some(limit) = query.limit {
        rows.truncate(limit);
    }
    let rows = rows.into_iter().map(|row| row.into_iter().map(|term| term.as_ref().clone()).collect()).collect();
    Ok((columns, rows))
}

fn decode(row: &Row, terms: &mut Terms<'_>) -> Result<TermRow> {
    row.iter().map(|id| terms.term(*id)).collect()
}

fn ordering(order_by: &[Order], left: &TermRow, right: &TermRow) -> Ordering {
    for order in order_by {
        let found = left[order.column].cmp(&right[order.column]);
        let found = if order.descending { found.reverse() } else { found };
        if found != Ordering::Equal {
            return found;
        }
    }
    left.cmp(right)
}

fn aggregate(find: &AggregateFind, relation: &DerivedRelation, terms: &mut Terms<'_>) -> Result<Vec<TermRow>> {
    let mut groups: BTreeMap<Vec<i64>, Vec<&Row>> = BTreeMap::new();
    for row in relation.rows() {
        groups.entry(find.group.iter().map(|column| row[*column]).collect()).or_default().push(row);
    }
    let mut out = Vec::new();
    for (key, rows) in groups {
        let mut results = Vec::with_capacity(find.aggregates.len());
        for aggregate in &find.aggregates {
            results.push(compute(aggregate.op, aggregate.column, &rows, terms)?);
        }
        let kept = find.having.iter().all(|having| ops::compare(having.op, &results[having.aggregate], &having.value));
        if kept {
            let mut row: TermRow = key.iter().map(|id| terms.term(*id)).collect::<Result<_>>()?;
            row.extend(results.into_iter().map(Rc::new));
            out.push(row);
        }
    }
    Ok(out)
}

fn compute(op: AggregateOp, column: Option<usize>, rows: &[&Row], terms: &mut Terms<'_>) -> Result<Term> {
    let mut selected = |column: usize| rows.iter().map(|row| terms.term(row[column])).collect::<Result<Vec<_>>>();
    let column = column.unwrap_or(0);
    match op {
        AggregateOp::Count => Ok(Term::int(count(rows.len()))),
        AggregateOp::CountDistinct => Ok(Term::int(count(rows.iter().map(|row| row[column]).collect::<HashSet<_>>().len()))),
        AggregateOp::Min | AggregateOp::Max => {
            let values = selected(column)?;
            let by_value = |left: &Rc<Term>, right: &Rc<Term>| numeric_cmp(left, right).unwrap_or_else(|| left.cmp(right));
            let best = if op == AggregateOp::Min { values.into_iter().min_by(by_value) } else { values.into_iter().max_by(by_value) };
            best.map(|term| term.as_ref().clone()).ok_or_else(|| QueryError::Invalid("min and max need at least one row".to_string()).into())
        }
        AggregateOp::Sum | AggregateOp::Avg => numeric(op, &selected(column)?),
    }
}

fn count(length: usize) -> i64 {
    i64::try_from(length).unwrap_or(i64::MAX)
}

fn numeric(op: AggregateOp, values: &[Rc<Term>]) -> Result<Term> {
    if let Some(bad) = values.iter().find(|term| !term.is_number()) {
        return Err(QueryError::Invalid(format!("a numeric aggregate met non-numeric value {bad}")).into());
    }
    if op == AggregateOp::Avg {
        let total: f64 = values.iter().filter_map(|term| ops::number(term)).sum();
        return Ok(Term::float(if values.is_empty() { 0.0 } else { total / values.len() as f64 }));
    }
    let mut total = Term::int(0);
    for value in values {
        total = ops::arithmetic(super::ast::Arith::Add, &total, value).ok_or_else(|| QueryError::Invalid("sum overflowed".to_string()))?;
    }
    Ok(total)
}
