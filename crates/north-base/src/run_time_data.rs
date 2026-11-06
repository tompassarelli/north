use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::build_inputs::{run_time_data, rust_build_input};
use crate::platform::git;
use crate::platform::harness::DEFAULTS;

mod allowed;

fn git_text(directory: &Path, arguments: &[&str]) -> Option<String> {
    git::run(directory, arguments).ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A run-time file's own reader: `validate` is the parse and checks the running north applies to that file's text before using it.
pub struct Consumer {
    pub path: &'static str,
    pub validate: fn(&str) -> Result<(), String>,
}

static CONSUMERS: OnceLock<&'static [Consumer]> = OnceLock::new();

pub fn install(consumers: &'static [Consumer]) {
    let _ = CONSUMERS.set(consumers);
}

const USAGE: &str = "usage: north land scan run-time-data BASE..HEAD";

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub key: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    String,
    Integer,
    Float,
    Boolean,
    Datetime,
    Array,
    Table,
}

#[derive(Clone, Copy)]
enum Node<'a> {
    Item(&'a Item),
    Table(&'a Table),
    Value(&'a Value),
}

impl<'a> Node<'a> {
    fn kind(self) -> Option<Kind> {
        let value = match self {
            Node::Item(Item::None) => return None,
            Node::Item(Item::Table(_)) | Node::Table(_) => return Some(Kind::Table),
            Node::Item(Item::ArrayOfTables(_)) => return Some(Kind::Array),
            Node::Item(Item::Value(value)) | Node::Value(value) => value,
        };
        Some(match value {
            Value::String(_) => Kind::String,
            Value::Integer(_) => Kind::Integer,
            Value::Float(_) => Kind::Float,
            Value::Boolean(_) => Kind::Boolean,
            Value::Datetime(_) => Kind::Datetime,
            Value::Array(_) => Kind::Array,
            Value::InlineTable(_) => Kind::Table,
        })
    }

    fn keys(self) -> Vec<(&'a str, Node<'a>)> {
        match self {
            Node::Item(Item::Table(table)) | Node::Table(table) => table.iter().map(|(key, item)| (key, Node::Item(item))).collect(),
            Node::Item(Item::Value(Value::InlineTable(table))) | Node::Value(Value::InlineTable(table)) => table.iter().map(|(key, value)| (key, Node::Value(value))).collect(),
            _ => Vec::new(),
        }
    }

    fn entries(self) -> Vec<Node<'a>> {
        match self {
            Node::Item(Item::Value(Value::Array(array))) | Node::Value(Value::Array(array)) => array.iter().map(Node::Value).collect(),
            Node::Item(Item::ArrayOfTables(tables)) => tables.iter().map(Node::Table).collect(),
            _ => Vec::new(),
        }
    }

    fn child(self, key: &str) -> Option<Node<'a>> {
        self.keys().into_iter().find_map(|(name, node)| (name == key).then_some(node)).filter(|node| node.kind().is_some())
    }
}

type Pair<'a> = (String, Node<'a>, Node<'a>);

fn compare(path: &str, at: &str, base: Node, head: Node, found: &mut Vec<Finding>) {
    let (Some(was), Some(now)) = (base.kind(), head.kind()) else {
        return;
    };
    let mut push = |key: String, reason: String| found.push(Finding { path: path.to_owned(), key, reason });
    if was != now {
        let hint = if matches!((was, now), (Kind::Integer, Kind::Float) | (Kind::Float, Kind::Integer)) { " (write the number in the type it had, such as 3.0 for a float)" } else { "" };
        push(at.to_owned(), format!("was {was:?}, now {now:?}{hint}"));
        return;
    }
    let place = |key: &str| if at.is_empty() { key.to_owned() } else { format!("{at}.{key}") };
    let mut nested = Vec::new();
    for (key, node) in base.keys() {
        match head.child(key) {
            Some(next) => nested.push((place(key), node, next)),
            None => push(place(key), "removed".to_owned()),
        }
    }
    nested.extend(entry_pairs(at, base, head, &mut push));
    for (at, was, now) in nested {
        compare(path, &at, was, now, found);
    }
}

fn entry_pairs<'a>(at: &str, base: Node<'a>, head: Node<'a>, push: &mut impl FnMut(String, String)) -> Vec<Pair<'a>> {
    let (before, after) = (base.entries(), head.entries());
    let Some(&first) = before.first() else {
        return Vec::new();
    };
    if after.is_empty() {
        push(at.to_owned(), "emptied, but its readers had entries".to_owned());
    }
    let common: BTreeSet<&str> = first.keys().into_iter().map(|(key, _)| key).filter(|key| before.iter().all(|entry| entry.child(key).is_some())).collect();
    let mut pairs = Vec::new();
    for (index, entry) in after.into_iter().enumerate() {
        let at = format!("{at}[{index}]");
        if entry.kind() != first.kind() {
            push(at, format!("entries were {:?}, this one is {:?}", first.kind(), entry.kind()));
            continue;
        }
        for key in &common {
            match (first.child(key), entry.child(key)) {
                (Some(witness), Some(next)) => pairs.push((format!("{at}.{key}"), witness, next)),
                _ => push(format!("{at}.{key}"), "every earlier entry had it".to_owned()),
            }
        }
    }
    pairs
}

pub fn check(path: &str, base: Option<&str>, head: Option<&str>) -> Vec<Finding> {
    let finding = |reason: String| vec![Finding { path: path.to_owned(), key: String::new(), reason }];
    let Some(head) = head else {
        return finding("deleted, but the running north reads it".to_owned());
    };
    if !path.ends_with(".toml") {
        return Vec::new();
    }
    let now = match head.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => return finding(format!("does not parse: {}", error.message())),
    };
    let was = base.and_then(|text| text.parse::<DocumentMut>().ok());
    let mut found = Vec::new();
    if let Some(was) = &was {
        compare(path, "", Node::Item(was.as_item()), Node::Item(now.as_item()), &mut found);
    }
    if path == DEFAULTS {
        found.extend(allowed::declared(path, was.as_ref(), &now));
    }
    for consumer in CONSUMERS.get().copied().unwrap_or_default().iter().filter(|consumer| consumer.path == path) {
        if let Err(reason) = (consumer.validate)(head) {
            found.push(Finding { path: path.to_owned(), key: String::new(), reason: format!("refused by the reader that loads it: {reason}") });
        }
    }
    found.sort_by(|left, right| left.key.cmp(&right.key));
    found
}

pub fn run(directory: &Path, range: &str) -> u8 {
    let Some((base, head)) = range.split_once("..").filter(|(base, head)| !base.is_empty() && !head.is_empty()) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let Some(listing) = git_text(directory, &["diff", "--name-only", "--no-renames", base, head]) else {
        eprintln!("run-time-data: cannot diff {range}");
        return 1;
    };
    let changed: Vec<&str> = listing.lines().collect();
    if let Some(code) = changed.iter().find(|path| rust_build_input(path)) {
        println!("run-time-data: {code} is a Rust build input, so the lane's Rust checks judge the data it reads");
        return 0;
    }
    let blob = |revision: &str, path: &str| git_text(directory, &["show", &format!("{revision}:{path}")]);
    let data: Vec<&str> = changed.into_iter().filter(|path| run_time_data(path)).collect();
    let found: Vec<Finding> = data.iter().flat_map(|path| check(path, blob(base, path).as_deref(), blob(head, path).as_deref())).collect();
    for finding in &found {
        eprintln!("run-time-data: {} {}: {}; a lane that changes no Rust code may only change values inside their allowed range and add keys", finding.path, finding.key, finding.reason);
    }
    if found.is_empty() {
        println!("run-time-data: {} run-time file(s) keep every key and type the running north reads, inside the values it accepts", data.len());
    }
    u8::from(!found.is_empty())
}

#[cfg(test)]
mod tests;
