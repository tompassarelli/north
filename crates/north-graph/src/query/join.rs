use std::fmt::Write as _;

use rusqlite::functions::FunctionFlags;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, params_from_iter};

use super::ast::Compare;
use super::base::{self, Window};
use super::budget::Budget;
use super::check::BaseRelation;
use super::ops;
use super::plan::Slot;
use crate::error::Result;
use crate::key::{Key, atom_key, decode_key};
use crate::store::Terms;
use crate::term::Term;

const COMPARE: &str = "north_compare";
const ORDERED: [Compare; 4] = [Compare::Lt, Compare::Le, Compare::Gt, Compare::Ge];

pub enum Member<'a> {
    Relation(BaseRelation, &'a [Slot]),
    Compare(Compare, Slot, Slot),
}

#[derive(Debug)]
enum Input {
    Id(usize),
    Number(usize),
}

#[derive(Debug)]
pub struct Join {
    sql: String,
    inputs: Vec<Input>,
    pub outputs: Vec<usize>,
    pub orders: bool,
}

pub struct Found {
    pub rows: usize,
    pub values: Vec<i64>,
}

struct Builder<'a> {
    bound_before: &'a [bool],
    columns: Vec<(usize, String)>,
    inputs: Vec<Input>,
    conditions: Vec<String>,
}

impl Builder<'_> {
    fn column(&self, variable: usize) -> Option<&str> {
        self.columns.iter().find(|(known, _)| *known == variable).map(|(_, column)| column.as_str())
    }

    fn input(&mut self, input: Input) -> String {
        self.inputs.push(input);
        format!("?{}", self.inputs.len())
    }

    fn id(&mut self, slot: Slot) -> String {
        match slot {
            Slot::Const(id) => id.to_string(),
            Slot::Var(variable) => match self.column(variable) {
                Some(column) => column.to_string(),
                None => self.input(Input::Id(variable)),
            },
        }
    }

    fn number(&mut self, slot: Slot, terms: &mut Terms<'_>) -> Result<String> {
        Ok(match slot {
            Slot::Const(id) => match number_key(&*terms.term(id)?) {
                Some(key) => key.iter().fold(String::from("X'"), |mut literal, byte| {
                    let _ = write!(literal, "{byte:02x}");
                    literal
                }) + "'",
                None => "NULL".to_string(),
            },
            Slot::Var(variable) => match self.column(variable) {
                Some(column) => format!("(SELECT key FROM terms WHERE id = {column})"),
                None => self.input(Input::Number(variable)),
            },
        })
    }

    fn relation(&mut self, relation: BaseRelation, args: &[Slot], window: Option<&Window>, alias: &str) -> String {
        let fragment = base::fragment(relation, window, alias);
        self.conditions.extend(fragment.conditions);
        for (column, slot) in fragment.columns.into_iter().zip(args) {
            match *slot {
                Slot::Var(variable) if !self.bound_before.get(variable).copied().unwrap_or(false) && self.column(variable).is_none() => {
                    self.columns.push((variable, column));
                }
                slot => {
                    let value = self.id(slot);
                    self.conditions.push(format!("{column} = {value}"));
                }
            }
        }
        fragment.from
    }

    fn compare(&mut self, op: Compare, left: Slot, right: Slot, terms: &mut Terms<'_>) -> Result<String> {
        Ok(match op {
            Compare::Eq => format!("{} = {}", self.id(left), self.id(right)),
            Compare::Ne => format!("{} <> {}", self.id(left), self.id(right)),
            ordered => {
                let code = ORDERED.iter().position(|known| *known == ordered).unwrap_or_default();
                format!("{COMPARE}({code}, {}, {})", self.number(left, terms)?, self.number(right, terms)?)
            }
        })
    }
}

pub fn compile(members: &[Member<'_>], bound_before: &[bool], needed: &[bool], window: Option<&Window>, terms: &mut Terms<'_>) -> Result<Join> {
    let mut builder = Builder { bound_before, columns: Vec::new(), inputs: Vec::new(), conditions: Vec::new() };
    let mut from = Vec::new();
    for (index, member) in members.iter().enumerate() {
        match member {
            Member::Relation(relation, args) => from.push(builder.relation(*relation, args, window, &format!("j{index}"))),
            Member::Compare(op, left, right) => {
                let condition = builder.compare(*op, *left, *right, terms)?;
                builder.conditions.push(condition);
            }
        }
    }
    let kept: Vec<&(usize, String)> = builder.columns.iter().filter(|(variable, _)| needed.get(*variable).copied().unwrap_or(false)).collect();
    let selected = if kept.is_empty() { "1".to_string() } else { kept.iter().map(|(_, column)| column.as_str()).collect::<Vec<_>>().join(", ") };
    let distinct = if !kept.is_empty() && kept.len() < builder.columns.len() { "DISTINCT " } else { "" };
    let mut sql = format!("SELECT {distinct}{selected} FROM {}", from.join(" CROSS JOIN "));
    if !builder.conditions.is_empty() {
        let _ = write!(sql, " WHERE {}", builder.conditions.join(" AND "));
    }
    if kept.is_empty() {
        sql.push_str(" LIMIT 1");
    }
    let outputs = kept.iter().map(|(variable, _)| *variable).collect();
    let orders = members.iter().any(|member| matches!(member, Member::Compare(op, ..) if ORDERED.contains(op)));
    Ok(Join { sql, inputs: builder.inputs, outputs, orders })
}

impl Join {
    pub fn fetch(&self, connection: &Connection, terms: &mut Terms<'_>, binding: &[i64], budget: &Budget) -> Result<Found> {
        let mut parameters = Vec::with_capacity(self.inputs.len());
        for input in &self.inputs {
            parameters.push(match *input {
                Input::Id(variable) => Value::Integer(binding[variable]),
                Input::Number(variable) => number_key(&*terms.term(binding[variable])?).map_or(Value::Null, Value::Blob),
            });
        }
        let width = self.outputs.len();
        let mut statement = connection.prepare_cached(&self.sql)?;
        let mut rows = statement.query(params_from_iter(parameters))?;
        let mut found = Found { rows: 0, values: Vec::new() };
        while let Some(row) = rows.next()? {
            budget.tick()?;
            found.rows += 1;
            for column in 0..width {
                found.values.push(row.get(column)?);
            }
        }
        Ok(found)
    }
}

fn number_key(term: &Term) -> Option<Vec<u8>> {
    match term {
        Term::Atom(atom) => Some(atom_key(atom)),
        Term::Triple(_) => None,
    }
}

fn atom_of(value: ValueRef<'_>) -> Option<Term> {
    let ValueRef::Blob(key) = value else { return None };
    match decode_key(key).ok()? {
        Key::Atom(atom) => Some(Term::Atom(atom)),
        Key::Triple(_) => None,
    }
}

pub fn register(connection: &Connection) -> Result<()> {
    connection.create_scalar_function(COMPARE, 3, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| {
        let code: i64 = context.get(0)?;
        let (Some(op), Some(left), Some(right)) = (usize::try_from(code).ok().and_then(|code| ORDERED.get(code)), atom_of(context.get_raw(1)), atom_of(context.get_raw(2))) else {
            return Ok(false);
        };
        Ok(ops::compare(*op, &left, &right))
    })?;
    Ok(())
}
