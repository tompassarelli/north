use serde_json::{Map, Value};

use super::ast::{Aggregate, AggregateFind, AggregateOp, Arg, Arith, Clause, Compare, Find, Having, Order, Query, Rule};
use crate::error::QueryError;
use crate::json::term_from_json;

type Parsed<T> = Result<T, QueryError>;

fn invalid<T>(why: impl Into<String>) -> Parsed<T> {
    Err(QueryError::Invalid(why.into()))
}

pub fn parse_query(text: &str) -> Parsed<Query> {
    let value: Value = serde_json::from_str(text).map_err(|why| QueryError::Invalid(format!("query is not JSON: {why}")))?;
    from_value(&value)
}

pub fn from_value(value: &Value) -> Parsed<Query> {
    let object = object(value, "query", &["find", "rules", "strata", "orderBy", "limit", "timeoutMs"])?;
    let find = match object.get("find") {
        Some(Value::String(name)) => Find::Relation(name.clone()),
        Some(value) => Find::Aggregate(aggregate_find(value)?),
        None => return invalid("query needs find"),
    };
    let strata = match (object.get("rules"), object.get("strata")) {
        (Some(rules), None) => vec![rules_of(rules)?],
        (None, Some(Value::Array(strata))) => strata.iter().map(rules_of).collect::<Parsed<Vec<_>>>()?,
        _ => return invalid("query needs exactly one of rules or strata"),
    };
    let order_by = match object.get("orderBy") {
        Some(Value::Array(clauses)) => clauses.iter().map(order).collect::<Parsed<Vec<_>>>()?,
        Some(_) => return invalid("orderBy is a list"),
        None => Vec::new(),
    };
    let limit = object.get("limit").map(|value| unsigned(value, "limit")).transpose()?;
    let timeout_ms = object.get("timeoutMs").map(|value| unsigned(value, "timeoutMs")).transpose()?;
    Ok(Query { find, strata, order_by, limit: limit.map(|limit| limit as usize), timeout_ms })
}

fn object<'a>(value: &'a Value, what: &str, allowed: &[&str]) -> Parsed<&'a Map<String, Value>> {
    let Value::Object(map) = value else {
        return invalid(format!("{what} is an object"));
    };
    if let Some(unknown) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return invalid(format!("{what} has unknown field {unknown}"));
    }
    Ok(map)
}

fn unsigned(value: &Value, what: &str) -> Parsed<u64> {
    value.as_u64().map_or_else(|| invalid(format!("{what} is a non-negative integer")), Ok)
}

fn name<'a>(map: &'a Map<String, Value>, field: &str, what: &str) -> Parsed<&'a str> {
    map.get(field).and_then(Value::as_str).map_or_else(|| invalid(format!("{what} needs a string {field}")), Ok)
}

fn rules_of(value: &Value) -> Parsed<Vec<Rule>> {
    match value {
        Value::Array(rules) => rules.iter().map(rule).collect(),
        _ => invalid("rules is a list"),
    }
}

fn rule(value: &Value) -> Parsed<Rule> {
    let map = object(value, "rule", &["head", "body"])?;
    let head = object(map.get("head").unwrap_or(&Value::Null), "rule head", &["rel", "args"])?;
    let body = match map.get("body") {
        Some(Value::Array(clauses)) => clauses.iter().map(clause).collect::<Parsed<Vec<_>>>()?,
        _ => return invalid("rule needs a body list"),
    };
    Ok(Rule { head: name(head, "rel", "rule head")?.to_string(), head_args: args(head.get("args"))?, body })
}

fn args(value: Option<&Value>) -> Parsed<Vec<Arg>> {
    match value {
        Some(Value::Array(items)) => items.iter().map(arg).collect(),
        _ => invalid("args is a list"),
    }
}

fn arg(value: &Value) -> Parsed<Arg> {
    if let Value::Object(map) = value {
        let variable = object(value, "variable", &["var"])?;
        let _ = map;
        return match variable.get("var") {
            Some(Value::String(variable)) if !variable.is_empty() => Ok(Arg::Var(variable.clone())),
            _ => invalid("a variable is {\"var\": NAME}"),
        };
    }
    term_from_json(value).map(Arg::Const).map_err(|why| QueryError::Invalid(format!("bad constant: {why}")))
}

fn clause(value: &Value) -> Parsed<Clause> {
    let Value::Object(map) = value else {
        return invalid("a body clause is an object");
    };
    if map.contains_key("rel") {
        let map = object(value, "relation clause", &["rel", "args", "neg"])?;
        let negated = match map.get("neg") {
            None => false,
            Some(Value::Bool(flag)) => *flag,
            Some(_) => return invalid("neg is a boolean"),
        };
        return Ok(Clause::Relation { name: name(map, "rel", "relation clause")?.to_string(), args: args(map.get("args"))?, negated });
    }
    if map.contains_key("pred") {
        let map = object(value, "predicate clause", &["pred", "args"])?;
        let [left, right] = pair(map.get("args"), "predicate")?;
        return Ok(Clause::Predicate { op: compare(name(map, "pred", "predicate clause")?)?, left, right });
    }
    if map.contains_key("fn") {
        let map = object(value, "function clause", &["fn", "args", "bind"])?;
        let [left, right] = pair(map.get("args"), "function")?;
        return Ok(Clause::Function { op: arith(name(map, "fn", "function clause")?)?, left, right, bind: name(map, "bind", "function clause")?.to_string() });
    }
    invalid("a body clause has rel, pred or fn")
}

fn pair(value: Option<&Value>, what: &str) -> Parsed<[Arg; 2]> {
    let mut parsed = args(value)?;
    if parsed.len() != 2 {
        return invalid(format!("a {what} takes exactly two arguments"));
    }
    let right = parsed.remove(1);
    Ok([parsed.remove(0), right])
}

fn compare(op: &str) -> Parsed<Compare> {
    Ok(match op {
        "eq" => Compare::Eq,
        "ne" => Compare::Ne,
        "lt" => Compare::Lt,
        "le" => Compare::Le,
        "gt" => Compare::Gt,
        "ge" => Compare::Ge,
        other => return invalid(format!("unknown comparison {other}")),
    })
}

fn arith(op: &str) -> Parsed<Arith> {
    Ok(match op {
        "+" => Arith::Add,
        "-" => Arith::Sub,
        "*" => Arith::Mul,
        "/" => Arith::Div,
        "mod" => Arith::Mod,
        other => return invalid(format!("unknown arithmetic operation {other}")),
    })
}

fn order(value: &Value) -> Parsed<Order> {
    let map = object(value, "orderBy clause", &["column", "direction"])?;
    let column = unsigned(map.get("column").unwrap_or(&Value::Null), "orderBy column")? as usize;
    match name(map, "direction", "orderBy clause")? {
        "asc" => Ok(Order { column, descending: false }),
        "desc" => Ok(Order { column, descending: true }),
        other => invalid(format!("unknown direction {other}")),
    }
}

fn aggregate_find(value: &Value) -> Parsed<AggregateFind> {
    let map = object(value, "aggregate find", &["rel", "group", "agg", "having"])?;
    let indices = |field: &str| -> Parsed<Vec<usize>> {
        match map.get(field) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items.iter().map(|item| unsigned(item, field).map(|index| index as usize)).collect(),
            Some(_) => invalid(format!("{field} is a list")),
        }
    };
    let aggregates = match map.get("agg") {
        Some(Value::Array(items)) if !items.is_empty() => items.iter().map(aggregate).collect::<Parsed<Vec<_>>>()?,
        _ => return invalid("aggregate find needs a non-empty agg list"),
    };
    let having = match map.get("having") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(having).collect::<Parsed<Vec<_>>>()?,
        Some(_) => return invalid("having is a list"),
    };
    Ok(AggregateFind { relation: name(map, "rel", "aggregate find")?.to_string(), group: indices("group")?, aggregates, having })
}

fn aggregate(value: &Value) -> Parsed<Aggregate> {
    let map = object(value, "aggregate", &["op", "arg"])?;
    let op = match name(map, "op", "aggregate")? {
        "count" => AggregateOp::Count,
        "count-distinct" => AggregateOp::CountDistinct,
        "sum" => AggregateOp::Sum,
        "avg" => AggregateOp::Avg,
        "min" => AggregateOp::Min,
        "max" => AggregateOp::Max,
        other => return invalid(format!("unknown aggregate {other}")),
    };
    let column = map.get("arg").map(|value| unsigned(value, "aggregate arg").map(|column| column as usize)).transpose()?;
    if op != AggregateOp::Count && column.is_none() {
        return invalid("every aggregate except count needs arg");
    }
    Ok(Aggregate { op, column })
}

fn having(value: &Value) -> Parsed<Having> {
    let map = object(value, "having clause", &["op", "agg", "val"])?;
    let aggregate = unsigned(map.get("agg").unwrap_or(&Value::Null), "having agg")? as usize;
    let value = term_from_json(map.get("val").unwrap_or(&Value::Null)).map_err(|why| QueryError::Invalid(format!("bad having value: {why}")))?;
    Ok(Having { op: compare(name(map, "op", "having clause")?)?, aggregate, value })
}
