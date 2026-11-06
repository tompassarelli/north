use std::rc::Rc;

use rusqlite::Connection;

use super::base;
use super::budget::Budget;
use super::ops;
use super::plan::{Plan, RulePlan, Slot, Source, Step, StratumPlan};
use super::relation::{DerivedRelation, Row};
use crate::error::Result;
use crate::store::Terms;

const UNBOUND: i64 = 0;

pub struct Evaluator<'a, 'c> {
    pub connection: &'a Connection,
    pub terms: &'a mut Terms<'c>,
    pub budget: &'a Budget,
}

pub fn run(evaluator: &mut Evaluator<'_, '_>, plan: &Plan) -> Result<Vec<DerivedRelation>> {
    let mut full: Vec<DerivedRelation> = plan.masks.iter().map(|masks| DerivedRelation::new(masks)).collect();
    for stratum in &plan.strata {
        evaluator.stratum(plan, stratum, &mut full)?;
    }
    Ok(full)
}

impl Evaluator<'_, '_> {
    fn stratum(&mut self, plan: &Plan, stratum: &StratumPlan, full: &mut [DerivedRelation]) -> Result<()> {
        let fresh = || -> Vec<DerivedRelation> { plan.masks.iter().map(|masks| DerivedRelation::new(masks)).collect() };
        let no_delta = fresh();
        let mut delta = fresh();
        for rule in &stratum.rules {
            let produced = self.rule(rule, full, &no_delta, None)?;
            absorb(rule.head, produced, full, &mut delta);
        }
        while stratum.heads.iter().any(|head| !delta[*head].is_empty()) {
            let mut next = fresh();
            for rule in &stratum.rules {
                for (position, step) in rule.steps.iter().enumerate() {
                    let Step::Lookup { source: Source::Derived(read), negated: false, .. } = step else { continue };
                    if !stratum.heads.contains(read) || delta[*read].is_empty() {
                        continue;
                    }
                    let produced = self.rule(rule, full, &delta, Some(position))?;
                    absorb(rule.head, produced, full, &mut next);
                }
            }
            delta = next;
        }
        Ok(())
    }

    fn rule(&mut self, rule: &RulePlan, full: &[DerivedRelation], delta: &[DerivedRelation], delta_at: Option<usize>) -> Result<Vec<Row>> {
        let mut binding = vec![UNBOUND; rule.variables];
        let mut out = Vec::new();
        let scope = Scope { rule, full, delta, delta_at };
        self.join(&scope, 0, &mut binding, &mut out)?;
        Ok(out)
    }

    fn join(&mut self, scope: &Scope<'_>, at: usize, binding: &mut Vec<i64>, out: &mut Vec<Row>) -> Result<()> {
        let Some(step) = scope.rule.steps.get(at) else {
            self.budget.tick()?;
            out.push(scope.rule.head_args.iter().map(|slot| value(slot, binding)).collect::<Vec<_>>().into());
            return Ok(());
        };
        match step {
            Step::Lookup { source, args, negated } => self.lookup(scope, at, (*source, args, *negated), binding, out),
            Step::Compare { op, left, right } => {
                let (left, right) = (self.terms.term(value(left, binding))?, self.terms.term(value(right, binding))?);
                if ops::compare(*op, &left, &right) { self.join(scope, at + 1, binding, out) } else { Ok(()) }
            }
            Step::Compute { op, left, right, bind } => {
                let (left, right) = (self.terms.term(value(left, binding))?, self.terms.term(value(right, binding))?);
                let Some(result) = ops::arithmetic(*op, &left, &right) else { return Ok(()) };
                binding[*bind] = self.terms.id_of(&result)?;
                let outcome = self.join(scope, at + 1, binding, out);
                binding[*bind] = UNBOUND;
                outcome
            }
        }
    }

    fn lookup(&mut self, scope: &Scope<'_>, at: usize, read: (Source, &[Slot], bool), binding: &mut Vec<i64>, out: &mut Vec<Row>) -> Result<()> {
        let (source, args, negated) = read;
        let key: Vec<Option<i64>> = args.iter().map(|slot| Some(value(slot, binding)).filter(|id| *id != UNBOUND)).collect();
        let rows = match source {
            Source::Base(relation) => base::fetch(self.connection, relation, &key, self.budget)?,
            Source::Derived(id) => {
                let relation = if scope.delta_at == Some(at) { &scope.delta[id] } else { &scope.full[id] };
                relation.lookup(&key)
            }
        };
        if negated {
            return if rows.is_empty() { self.join(scope, at + 1, binding, out) } else { Ok(()) };
        }
        for row in rows {
            self.budget.tick()?;
            let mut newly: Vec<usize> = Vec::new();
            if unify(args, &row, binding, &mut newly) {
                self.join(scope, at + 1, binding, out)?;
            }
            for variable in newly {
                binding[variable] = UNBOUND;
            }
        }
        Ok(())
    }
}

struct Scope<'a> {
    rule: &'a RulePlan,
    full: &'a [DerivedRelation],
    delta: &'a [DerivedRelation],
    delta_at: Option<usize>,
}

fn value(slot: &Slot, binding: &[i64]) -> i64 {
    match slot {
        Slot::Const(id) => *id,
        Slot::Var(index) => binding[*index],
    }
}

fn unify(args: &[Slot], row: &[i64], binding: &mut [i64], newly: &mut Vec<usize>) -> bool {
    for (slot, found) in args.iter().zip(row) {
        match slot {
            Slot::Const(id) if id != found => return false,
            Slot::Var(index) if binding[*index] == UNBOUND => {
                binding[*index] = *found;
                newly.push(*index);
            }
            Slot::Var(index) if binding[*index] != *found => return false,
            _ => {}
        }
    }
    true
}

fn absorb(head: usize, produced: Vec<Row>, full: &mut [DerivedRelation], delta: &mut [DerivedRelation]) {
    for row in produced {
        if full[head].insert(Rc::clone(&row)) {
            delta[head].insert(row);
        }
    }
}
