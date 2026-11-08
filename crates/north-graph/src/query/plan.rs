use std::collections::HashMap;

use super::ast::{Arg, Arith, Clause, Compare, Query, Rule};
use super::base::Window;
use super::check::{BaseRelation, Schema};
use super::join::{self, Join, Member};
use super::slot::Slot;
use crate::error::Result;
use crate::store::Terms;

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
    Join(Join),
}

impl Step {
    fn slots(&self) -> Vec<Slot> {
        match self {
            Self::Lookup { args, .. } => args.clone(),
            Self::Compare { left, right, .. } => vec![*left, *right],
            Self::Compute { left, right, bind, .. } => vec![*left, *right, Slot::Var(*bind)],
            Self::Join(join) => join.outputs.iter().map(|variable| Slot::Var(*variable)).collect(),
        }
    }

    fn pushable(&self) -> bool {
        matches!(self, Self::Lookup { source: Source::Base(_), negated: false, .. })
    }
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

impl Plan {
    pub fn orders_in_sql(&self) -> bool {
        self.strata.iter().flat_map(|stratum| &stratum.rules).flat_map(|rule| &rule.steps).any(|step| matches!(step, Step::Join(join) if join.orders))
    }
}

pub fn lower(query: &Query, schema: &Schema, terms: &mut Terms<'_>, window: Option<&Window>) -> Result<Plan> {
    let mut masks = vec![Vec::new(); schema.derived.len()];
    let mut strata = Vec::with_capacity(query.strata.len());
    for rules in &query.strata {
        let mut plans = Vec::with_capacity(rules.len());
        for rule in rules {
            let mut plan = lower_rule(rule, schema, terms, &mut masks)?;
            plan.steps = push_down(std::mem::take(&mut plan.steps), &plan.head_args, plan.variables, window, terms)?;
            plans.push(plan);
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

fn push_down(steps: Vec<Step>, head_args: &[Slot], variables: usize, window: Option<&Window>, terms: &mut Terms<'_>) -> Result<Vec<Step>> {
    let mut read_from = vec![vec![false; variables]; steps.len() + 1];
    for slot in head_args {
        mark(&mut read_from[steps.len()], slot);
    }
    for (position, step) in steps.iter().enumerate().rev() {
        read_from[position] = read_from[position + 1].clone();
        for slot in step.slots() {
            mark(&mut read_from[position], &slot);
        }
    }
    let mut bound = vec![false; variables];
    let mut lowered = Vec::with_capacity(steps.len());
    let mut run: Vec<Step> = Vec::new();
    for (position, step) in steps.into_iter().enumerate() {
        if step.pushable() || (!run.is_empty() && matches!(step, Step::Compare { .. })) {
            run.push(step);
            continue;
        }
        if !run.is_empty() {
            lowered.push(compile_run(&std::mem::take(&mut run), &mut bound, &read_from[position], window, terms)?);
        }
        for slot in step.slots().iter().filter(|_| !matches!(step, Step::Lookup { negated: true, .. })) {
            mark(&mut bound, slot);
        }
        lowered.push(step);
    }
    if !run.is_empty() {
        let after = read_from.len() - 1;
        lowered.push(compile_run(&run, &mut bound, &read_from[after], window, terms)?);
    }
    Ok(lowered)
}

fn compile_run(run: &[Step], bound: &mut Vec<bool>, needed: &[bool], window: Option<&Window>, terms: &mut Terms<'_>) -> Result<Step> {
    let members: Vec<Member<'_>> = run
        .iter()
        .filter_map(|step| match step {
            Step::Lookup { source: Source::Base(relation), args, .. } => Some(Member::Relation(*relation, args)),
            Step::Compare { op, left, right } => Some(Member::Compare(*op, *left, *right)),
            _ => None,
        })
        .collect();
    let join = join::compile(&members, bound, needed, window, terms)?;
    for step in run {
        for slot in step.slots() {
            mark(bound, &slot);
        }
    }
    Ok(Step::Join(join))
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
