use std::collections::{HashMap, HashSet};

use super::ast::{Arg, Clause, Find, Query, Rule};
use crate::error::QueryError;

pub const MAX_LIMIT: usize = 100_000;
const TEXT_RELATIONS: [&str; 5] = ["text-match", "text-phrase", "text-substring", "text-stem", "text-search"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaseRelation {
    Triple,
    Occurrence,
    Withdrawal,
}

impl BaseRelation {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "triple" => Some(Self::Triple),
            "occurrence" => Some(Self::Occurrence),
            "withdrawal" => Some(Self::Withdrawal),
            _ => None,
        }
    }

    pub fn arity(self) -> usize {
        match self {
            Self::Triple | Self::Occurrence => 3,
            Self::Withdrawal => 2,
        }
    }
}

#[derive(Debug)]
pub struct Derived {
    pub arity: usize,
    pub stratum: usize,
}

#[derive(Debug, Default)]
pub struct Schema {
    pub derived: Vec<Derived>,
    pub ids: HashMap<String, usize>,
}

impl Schema {
    pub fn arity_of(&self, name: &str) -> Option<usize> {
        BaseRelation::named(name).map(BaseRelation::arity).or_else(|| self.ids.get(name).map(|id| self.derived[*id].arity))
    }
}

type Checked<T> = Result<T, QueryError>;

fn invalid<T>(why: String) -> Checked<T> {
    Err(QueryError::Invalid(why))
}

pub fn check(query: &Query) -> Checked<Schema> {
    let schema = declare(query)?;
    for (stratum, rules) in query.strata.iter().enumerate() {
        for rule in rules {
            check_rule(&schema, stratum, rule)?;
        }
        check_recursion(&schema, rules)?;
    }
    check_find(&schema, query)?;
    Ok(schema)
}

fn declare(query: &Query) -> Checked<Schema> {
    let mut schema = Schema::default();
    for (stratum, rules) in query.strata.iter().enumerate() {
        for rule in rules {
            if BaseRelation::named(&rule.head).is_some() || TEXT_RELATIONS.contains(&rule.head.as_str()) {
                return invalid(format!("rule head {} names a stored relation", rule.head));
            }
            let arity = rule.head_args.len();
            match schema.ids.get(&rule.head) {
                Some(id) => {
                    let known = &schema.derived[*id];
                    if known.arity != arity {
                        return invalid(format!("relation {} has heads of arity {} and {arity}", rule.head, known.arity));
                    }
                    if known.stratum != stratum {
                        return invalid(format!("relation {} is defined in strata {} and {stratum}", rule.head, known.stratum));
                    }
                }
                None => {
                    schema.ids.insert(rule.head.clone(), schema.derived.len());
                    schema.derived.push(Derived { arity, stratum });
                }
            }
        }
    }
    Ok(schema)
}

fn check_rule(schema: &Schema, stratum: usize, rule: &Rule) -> Checked<()> {
    let mut bound: HashSet<&str> = HashSet::new();
    for clause in &rule.body {
        match clause {
            Clause::Relation { name, args, negated } => {
                check_reference(schema, stratum, name, args.len(), *negated)?;
                for arg in args {
                    match (arg, negated) {
                        (Arg::Var(variable), false) => {
                            bound.insert(variable);
                        }
                        (Arg::Var(variable), true) => require_bound(&bound, variable, "a negated clause")?,
                        _ => {}
                    }
                }
            }
            Clause::Predicate { left, right, .. } => {
                require_arg(&bound, left, "a comparison")?;
                require_arg(&bound, right, "a comparison")?;
            }
            Clause::Function { left, right, bind, .. } => {
                require_arg(&bound, left, "arithmetic")?;
                require_arg(&bound, right, "arithmetic")?;
                if !bound.insert(bind) {
                    return invalid(format!("arithmetic binds {bind}, which is already bound"));
                }
            }
        }
    }
    for arg in &rule.head_args {
        require_arg(&bound, arg, "the head of the rule")?;
    }
    Ok(())
}

fn require_arg(bound: &HashSet<&str>, arg: &Arg, what: &str) -> Checked<()> {
    match arg {
        Arg::Var(variable) => require_bound(bound, variable, what),
        Arg::Const(_) => Ok(()),
    }
}

fn require_bound(bound: &HashSet<&str>, variable: &str, what: &str) -> Checked<()> {
    if bound.contains(variable) { Ok(()) } else { invalid(format!("variable {variable} is unbound where {what} reads it")) }
}

fn check_reference(schema: &Schema, stratum: usize, name: &str, arity: usize, negated: bool) -> Checked<()> {
    if TEXT_RELATIONS.contains(&name) {
        return invalid(format!("text relation {name} is not supported"));
    }
    let Some(expected) = schema.arity_of(name) else {
        return invalid(format!("relation {name} is neither stored nor defined by a rule"));
    };
    if expected != arity {
        return invalid(format!("relation {name} has arity {expected}, used with {arity}"));
    }
    if let Some(id) = schema.ids.get(name) {
        let defined = schema.derived[*id].stratum;
        if defined > stratum || (negated && defined == stratum) {
            let why = if negated { "a negated clause needs its relation in an earlier stratum" } else { "a clause may only read relations from its own or an earlier stratum" };
            return invalid(format!("relation {name}: {why}"));
        }
    }
    Ok(())
}

fn check_recursion(schema: &Schema, rules: &[Rule]) -> Checked<()> {
    let mut reads: HashMap<&str, HashSet<&str>> = HashMap::new();
    for rule in rules {
        for clause in &rule.body {
            if let Clause::Relation { name, negated: false, .. } = clause
                && schema.ids.contains_key(name) {
                reads.entry(rule.head.as_str()).or_default().insert(name);
            }
        }
    }
    for rule in rules.iter().filter(|rule| rule.body.iter().any(|clause| matches!(clause, Clause::Function { .. }))) {
        let own_reads = rule.body.iter().filter_map(|clause| match clause {
            Clause::Relation { name, negated: false, .. } if schema.ids.contains_key(name) => Some(name.as_str()),
            _ => None,
        });
        let recursive = own_reads.into_iter().any(|read| reaches(&reads, read, &rule.head));
        if recursive {
            return invalid(format!("recursive rule for {} may not contain arithmetic", rule.head));
        }
    }
    Ok(())
}

fn reaches(reads: &HashMap<&str, HashSet<&str>>, from: &str, target: &str) -> bool {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack = vec![from];
    while let Some(current) = stack.pop() {
        if current == target {
            return true;
        }
        if seen.insert(current)
            && let Some(next) = reads.get(current) {
            stack.extend(next.iter().copied());
        }
    }
    false
}

fn check_find(schema: &Schema, query: &Query) -> Checked<()> {
    let (relation, output) = match &query.find {
        Find::Relation(name) => (name, None),
        Find::Aggregate(find) => (&find.relation, Some(find)),
    };
    let Some(arity) = schema.ids.get(relation).map(|id| schema.derived[*id].arity) else {
        return invalid(format!("find names {relation}, which no rule defines"));
    };
    let columns = match output {
        None => arity,
        Some(find) => {
            let selected = find.group.iter().chain(find.aggregates.iter().filter_map(|aggregate| aggregate.column.as_ref()));
            if let Some(column) = selected.into_iter().find(|column| **column >= arity) {
                return invalid(format!("column {column} is outside {relation}/{arity}"));
            }
            if let Some(having) = find.having.iter().find(|having| having.aggregate >= find.aggregates.len()) {
                return invalid(format!("having names aggregate {}, but only {} are computed", having.aggregate, find.aggregates.len()));
            }
            find.group.len() + find.aggregates.len()
        }
    };
    if let Some(order) = query.order_by.iter().find(|order| order.column >= columns) {
        return invalid(format!("orderBy column {} is outside the {columns} result columns", order.column));
    }
    match query.limit {
        Some(limit) if limit == 0 || limit > MAX_LIMIT => invalid(format!("limit is 1 through {MAX_LIMIT}")),
        _ => Ok(()),
    }
}
