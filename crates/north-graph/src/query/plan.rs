use std::collections::HashMap;

use super::ast::{Arg, Arith, Clause, Compare, Query, Rule};
use super::check::{BaseRelation, Schema};
use crate::error::Result;
use crate::store::Terms;

#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Var(usize),
    Const(i64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Base(BaseRelation),
    Derived(usize),
}

#[derive(Debug)]
pub enum Step {
    Lookup { source: Source, args: Vec<Slot>, negated: bool },
    Compare { op: Compare, left: Slot, right: Slot },
    Compute { op: Arith, left: Slot, right: Slot, bind: usize },
}

#[derive(Debug)]
pub struct RulePlan {
    pub head: usize,
    pub head_args: Vec<Slot>,
    pub steps: Vec<Step>,
    pub variables: usize,
}

#[derive(Debug)]
pub struct StratumPlan {
    pub rules: Vec<RulePlan>,
    pub heads: Vec<usize>,
}

#[derive(Debug)]
pub struct Plan {
    pub strata: Vec<StratumPlan>,
    pub masks: Vec<Vec<Vec<usize>>>,
}

pub fn lower(query: &Query, schema: &Schema, terms: &mut Terms<'_>) -> Result<Plan> {
    let mut masks = vec![Vec::new(); schema.derived.len()];
    let mut strata = Vec::with_capacity(query.strata.len());
    for rules in &query.strata {
        let mut plans = Vec::with_capacity(rules.len());
        for rule in rules {
            plans.push(lower_rule(rule, schema, terms, &mut masks)?);
        }
        let mut heads: Vec<usize> = plans.iter().map(|plan| plan.head).collect();
        heads.sort_unstable();
        heads.dedup();
        strata.push(StratumPlan { rules: plans, heads });
    }
    Ok(Plan { strata, masks })
}

struct Variables<'a> {
    slots: HashMap<&'a str, usize>,
}

impl<'a> Variables<'a> {
    fn slot(&mut self, arg: &'a Arg, terms: &mut Terms<'_>) -> Result<Slot> {
        match arg {
            Arg::Const(term) => Ok(Slot::Const(terms.id_of(term)?)),
            Arg::Var(name) => Ok(Slot::Var(self.variable(name))),
        }
    }

    fn variable(&mut self, name: &'a str) -> usize {
        let next = self.slots.len();
        *self.slots.entry(name).or_insert(next)
    }
}

fn lower_rule(rule: &Rule, schema: &Schema, terms: &mut Terms<'_>, masks: &mut [Vec<Vec<usize>>]) -> Result<RulePlan> {
    let mut variables = Variables { slots: HashMap::new() };
    let mut seen: Vec<bool> = Vec::new();
    let mut steps = Vec::with_capacity(rule.body.len());
    for clause in &rule.body {
        steps.push(match clause {
            Clause::Relation { name, args, negated } => {
                let source = BaseRelation::named(name).map_or_else(|| Source::Derived(schema.ids[name.as_str()]), Source::Base);
                let args = args.iter().map(|arg| variables.slot(arg, terms)).collect::<Result<Vec<_>>>()?;
                let bound: Vec<bool> = args.iter().map(|slot| is_bound(slot, &seen)).collect();
                for slot in &args {
                    mark(&mut seen, slot);
                }
                if let Source::Derived(id) = source {
                    let mask: Vec<usize> = bound.iter().enumerate().filter(|(_, bound)| **bound).map(|(column, _)| column).collect();
                    if !mask.is_empty() && !masks[id].contains(&mask) {
                        masks[id].push(mask);
                    }
                }
                Step::Lookup { source, args, negated: *negated }
            }
            Clause::Predicate { op, left, right } => Step::Compare { op: *op, left: variables.slot(left, terms)?, right: variables.slot(right, terms)? },
            Clause::Function { op, left, right, bind } => {
                let (left, right) = (variables.slot(left, terms)?, variables.slot(right, terms)?);
                let bind = variables.variable(bind);
                mark(&mut seen, &Slot::Var(bind));
                Step::Compute { op: *op, left, right, bind }
            }
        });
    }
    let head_args = rule.head_args.iter().map(|arg| variables.slot(arg, terms)).collect::<Result<Vec<_>>>()?;
    Ok(RulePlan { head: schema.ids[rule.head.as_str()], head_args, steps, variables: variables.slots.len() })
}

fn is_bound(slot: &Slot, seen: &[bool]) -> bool {
    match slot {
        Slot::Const(_) => true,
        Slot::Var(index) => seen.get(*index).copied().unwrap_or(false),
    }
}

fn mark(seen: &mut Vec<bool>, slot: &Slot) {
    if let Slot::Var(index) = slot {
        if seen.len() <= *index {
            seen.resize(*index + 1, false);
        }
        seen[*index] = true;
    }
}
