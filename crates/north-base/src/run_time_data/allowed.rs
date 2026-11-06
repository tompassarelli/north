use toml_edit::{DocumentMut, Item, Value};

use super::{Finding, Node};

const CONSTRAINTS: [&str; 7] = ["min", "max", "min_len", "max_len", "ascending", "one_of", "prefix"];

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(whole) => Some(*whole.value() as f64),
        Value::Float(real) => Some(*real.value()),
        _ => None,
    }
}

fn limit(allowed: Node, key: &str) -> Result<Option<f64>, String> {
    match allowed.child(key) {
        None => Ok(None),
        Some(Node::Item(Item::Value(value))) | Some(Node::Value(value)) => number(value).map(Some).ok_or_else(|| format!("allowed.{key} is not a number")),
        Some(_) => Err(format!("allowed.{key} is not a number")),
    }
}

fn leaves(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(array) => array.iter().collect(),
        other => vec![other],
    }
}

fn texts(allowed: Node, key: &str) -> Result<Option<Vec<String>>, String> {
    match allowed.child(key) {
        None => Ok(None),
        Some(Node::Item(Item::Value(Value::Array(array)))) | Some(Node::Value(Value::Array(array))) => array.iter().map(|entry| entry.as_str().map(str::to_owned).ok_or_else(|| format!("allowed.{key} holds a non-string"))).collect::<Result<_, _>>().map(Some),
        Some(_) => Err(format!("allowed.{key} is not a list of strings")),
    }
}

fn text(allowed: Node, key: &str) -> Result<Option<String>, String> {
    match allowed.child(key) {
        None => Ok(None),
        Some(Node::Item(Item::Value(Value::String(text)))) | Some(Node::Value(Value::String(text))) => Ok(Some(text.value().clone())),
        Some(_) => Err(format!("allowed.{key} is not a string")),
    }
}

pub(super) fn violations(value: &Value, allowed: Node) -> Vec<String> {
    let mut found = Vec::new();
    for (key, _) in allowed.keys() {
        if !CONSTRAINTS.contains(&key) {
            found.push(format!("allowed names {key}, which no reader of it checks"));
        }
    }
    if !found.is_empty() {
        return found;
    }
    match check(value, allowed) {
        Ok(()) => Vec::new(),
        Err(why) => vec![why],
    }
}

fn check(value: &Value, allowed: Node) -> Result<(), String> {
    let (min, max) = (limit(allowed, "min")?, limit(allowed, "max")?);
    let (min_len, max_len) = (limit(allowed, "min_len")?, limit(allowed, "max_len")?);
    let (one_of, prefix) = (texts(allowed, "one_of")?, text(allowed, "prefix")?);
    let items = leaves(value);
    if let Value::Array(array) = value {
        let len = array.len() as f64;
        if min_len.is_some_and(|floor| len < floor) || max_len.is_some_and(|ceiling| len > ceiling) {
            return Err(format!("holds {} entries, allowed {}..{}", array.len(), min_len.map_or("0".to_owned(), |floor| floor.to_string()), max_len.map_or("any".to_owned(), |ceiling| ceiling.to_string())));
        }
        if allowed.child("ascending").and_then(|node| node_bool(node)).unwrap_or(false) {
            let numbers: Vec<f64> = array.iter().filter_map(number).collect();
            if numbers.windows(2).any(|pair| pair[0] > pair[1]) {
                return Err("entries must not decrease".to_owned());
            }
        }
    } else if min_len.is_some() || max_len.is_some() {
        return Err("allowed bounds a length, but the value is not a list".to_owned());
    }
    for leaf in items {
        if min.is_some() || max.is_some() {
            let Some(real) = number(leaf) else {
                return Err("allowed bounds a number, but the value holds another type".to_owned());
            };
            if real.is_nan() || min.is_some_and(|floor| real < floor) || max.is_some_and(|ceiling| real > ceiling) {
                return Err(format!("{real} is outside {}..{}", min.map_or("-inf".to_owned(), |floor| floor.to_string()), max.map_or("inf".to_owned(), |ceiling| ceiling.to_string())));
            }
        }
        if one_of.is_some() || prefix.is_some() {
            let Some(word) = leaf.as_str() else {
                return Err("allowed bounds a string, but the value holds another type".to_owned());
            };
            if one_of.as_ref().is_some_and(|set| !set.iter().any(|choice| choice == word)) {
                return Err(format!("{word:?} is not one of {:?}", one_of.unwrap_or_default()));
            }
            if prefix.as_ref().is_some_and(|start| !word.starts_with(start.as_str())) {
                return Err(format!("{word:?} does not start with {:?}", prefix.unwrap_or_default()));
            }
        }
    }
    Ok(())
}

fn node_bool(node: Node) -> Option<bool> {
    match node {
        Node::Item(Item::Value(value)) | Node::Value(value) => value.as_bool(),
        _ => None,
    }
}

fn same(left: Node, right: Node) -> bool {
    let (mine, theirs) = (left.keys(), right.keys());
    if !mine.is_empty() || !theirs.is_empty() {
        return mine.len() == theirs.len() && mine.iter().all(|(key, node)| right.child(key).is_some_and(|other| same(*node, other)));
    }
    let (Some(Value::Array(first)), Some(Value::Array(second))) = (value_of(left), value_of(right)) else {
        return match (value_of(left), value_of(right)) {
            (Some(first), Some(second)) => number(first).zip(number(second)).map_or_else(|| first.as_str() == second.as_str() && first.as_bool() == second.as_bool(), |(one, other)| one == other),
            _ => false,
        };
    };
    first.len() == second.len() && first.iter().zip(second.iter()).all(|(one, other)| same(Node::Value(one), Node::Value(other)))
}

fn value_of<'a>(node: Node<'a>) -> Option<&'a Value> {
    match node {
        Node::Item(Item::Value(value)) | Node::Value(value) => Some(value),
        _ => None,
    }
}

pub(super) fn declared(path: &str, base: Option<&DocumentMut>, head: &DocumentMut) -> Vec<Finding> {
    let mut found = Vec::new();
    for (name, item) in head.iter() {
        let Some(entry) = item.as_table() else { continue };
        let Some(Item::Value(value)) = entry.get("value") else { continue };
        let mut push = |reason: String| found.push(Finding { path: path.to_owned(), key: name.to_owned(), reason });
        let was = base.and_then(|document| document.get(name)).and_then(Item::as_table).and_then(|table| table.get("allowed"));
        let now = entry.get("allowed");
        let Some(rule) = was.or(now) else {
            push("has a value but declares no allowed range or values beside it".to_owned());
            continue;
        };
        if let (Some(old), Some(new)) = (was, now)
            && !same(Node::Item(old), Node::Item(new))
        {
            push("changed its allowed range, which only the Rust that reads it may widen".to_owned());
            continue;
        }
        for why in violations(value, Node::Item(rule)) {
            push(format!("value {why}"));
        }
    }
    found
}
