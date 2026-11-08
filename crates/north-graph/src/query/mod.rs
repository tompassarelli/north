mod answer;
mod ast;
mod base;
mod budget;
mod check;
mod eval;
mod join;
mod ops;
mod parse;
mod plan;
mod relation;
mod slot;
mod text;

use crate::error::Result;
use crate::store::{Graph, Terms};

pub use answer::Answer;
pub use ast::Query;
pub use budget::{Budget, DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS, STEP_BUDGET};
pub use parse::parse_query;

impl Graph {
    pub fn ask(&self, query: &Query) -> Result<Answer> {
        self.ask_within(query, Budget::new(query.timeout_ms))
    }

    pub fn ask_within(&self, query: &Query, budget: Budget) -> Result<Answer> {
        let schema = check::check(query)?;
        let snapshot = self.connection().unchecked_transaction()?;
        let mut terms = Terms::new(&snapshot);
        let window = base::Window::resolve(&snapshot, &mut terms, query.view)?;
        let plan = plan::lower(query, &schema, &mut terms, window.as_ref())?;
        if plan.orders_in_sql() {
            join::register(&snapshot)?;
        }
        let mut evaluator = eval::Evaluator { connection: &snapshot, terms: &mut terms, budget: &budget, window: window.as_ref() };
        let relations = eval::run(&mut evaluator, &plan)?;
        let relation = match &query.find {
            ast::Find::Relation(name) => schema.ids[name.as_str()],
            ast::Find::Aggregate(find) => schema.ids[find.relation.as_str()],
        };
        let (columns, rows) = answer::shape(query, schema.derived[relation].arity, &relations[relation], &mut terms)?;
        Ok(Answer { columns, rows, steps: budget.steps() })
    }
}
