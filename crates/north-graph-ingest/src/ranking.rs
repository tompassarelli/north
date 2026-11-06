use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::boxes;
use crate::facts::{Facts, Sourced, text};
use crate::issues;
use crate::places::Places;

const WRITER: &str = "ranking";
const REPLACED: [&str; 2] = ["rank", "doc-waits-on"];

fn ranked_line() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^(\d+)\.\s+([A-Za-z][A-Za-z0-9_-]*#\d+)\b").unwrap_or_else(|error| panic!("{error}")))
}

fn side() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^([A-Za-z][A-Za-z0-9_-]*#\d+)(?:\s+box(?:es)?\s+(\d+(?:(?:\s*,\s*(?:and\s+)?|\s+and\s+)\d+)*))?\s*(?:\(.*\))?$").unwrap_or_else(|error| panic!("{error}"))
    })
}

pub fn read(places: &Places) -> Result<Sourced, String> {
    let document = std::fs::read_to_string(&places.ranking).map_err(|why| format!("{}: {why}", places.ranking.display()))?;
    let bodies = issues::bodies(places);
    let mut facts = Facts::new(&REPLACED);
    let mut in_dag = false;
    for line in document.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_dag = heading.trim().eq_ignore_ascii_case("DAG");
        }
        if let Some(found) = ranked_line().captures(line) {
            if let Ok(rank) = found[1].parse::<i64>() {
                facts.tell(WRITER, text(&found[2]), "rank", north_graph::Term::int(rank));
            }
        } else if in_dag && line.contains("->") {
            tell_chain(&mut facts, &bodies, line.trim_start_matches(['-', ' ']));
        }
    }
    Ok(facts.finish_noted())
}

fn nodes(bodies: &BTreeMap<String, String>, text: &str) -> Option<Vec<String>> {
    let found = side().captures(text.trim())?;
    let issue = found[1].to_owned();
    let Some(positions) = found.get(2) else { return Some(vec![issue]) };
    let items = boxes::parse(bodies.get(&issue)?);
    positions
        .as_str()
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| items.get(part.parse::<usize>().ok()?.checked_sub(1)?).map(|item| item.id(&issue)))
        .collect()
}

fn tell_chain(facts: &mut Facts, bodies: &BTreeMap<String, String>, line: &str) {
    let sides: Vec<&str> = line.split("->").collect();
    let parsed: Option<Vec<Vec<String>>> = sides.iter().map(|part| nodes(bodies, part)).collect();
    let Some(parsed) = parsed else {
        facts.note(format!("cannot parse a wait in backlog-ranking.md: {line}"));
        return;
    };
    for pair in parsed.windows(2) {
        for dependent in &pair[1] {
            for prerequisite in &pair[0] {
                facts.tell(WRITER, text(dependent), "doc-waits-on", text(prerequisite));
            }
        }
    }
}
