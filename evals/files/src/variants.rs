use std::collections::BTreeSet;

use serde_json::Value;

use crate::cap;

pub const GRAPH: &str = "graph alone";
pub const GREP: &str = "text search alone";
pub const MAP: &str = "classifier on brief + repository map";
pub const CANDIDATES: &str = "classifier on brief + candidates";

pub struct Ranked {
    pub name: String,
    pub files: Vec<String>,
    pub visible: BTreeSet<String>,
    pub classifier: Option<Value>,
    pub invented: usize,
}

const PLANS: [(&str, &[&str], bool); 5] = [
    ("map + text search", &[MAP, GREP], false),
    ("candidates + text search", &[CANDIDATES, GREP], false),
    ("map + candidates + text search", &[MAP, CANDIDATES, GREP], false),
    ("map + candidates + text search + graph", &[MAP, CANDIDATES, GREP, GRAPH], false),
    ("round-robin of map, candidates, text search, graph", &[MAP, CANDIDATES, GREP, GRAPH], true),
];

pub fn unions(sources: &[Ranked]) -> Vec<Ranked> {
    PLANS.iter().map(|(name, parts, round_robin)| {
        let chosen: Vec<&Ranked> = parts.iter().filter_map(|part| sources.iter().find(|source| source.name == *part)).collect();
        let lists: Vec<&[String]> = chosen.iter().map(|source| source.files.as_slice()).collect();
        let files = if *round_robin { cap::interleave(&lists) } else { cap::union_in_order(&lists) };
        let visible = chosen.iter().flat_map(|source| source.visible.iter().cloned()).collect();
        Ranked { name: (*name).to_owned(), files, visible, classifier: None, invented: 0 }
    }).collect()
}

