use std::collections::BTreeMap;
use std::path::Path;

use crate::threads::runs::hook_rules::HABIT_RULES;

#[derive(Debug, PartialEq)]
pub struct Row {
    pub rule: String,
    pub rewrites: usize,
    pub warnings: usize,
}

impl Row {
    pub fn total(&self) -> usize {
        self.rewrites + self.warnings
    }
}

pub fn rows(path: &Path, since: &str, until: &str) -> Result<Vec<Row>, String> {
    let db = super::scoreboard::open(path)?;
    let events = crate::threads::runs::hooks::load(&db, since)?;
    let mut grouped: BTreeMap<&str, Row> = BTreeMap::new();
    for event in events.iter().filter(|event| event.at.as_str() < until).filter(|event| HABIT_RULES.contains(&event.rule.as_str())) {
        let row = grouped.entry(event.rule.as_str()).or_insert_with(|| Row { rule: event.rule.clone(), rewrites: 0, warnings: 0 });
        match event.verdict.as_str() {
            "rewrite" => row.rewrites += 1,
            "advisory" => row.warnings += 1,
            _ => {}
        }
    }
    let mut rows: Vec<Row> = grouped.into_values().filter(|row| row.total() > 0).collect();
    rows.sort_by(|a, b| b.total().cmp(&a.total()).then_with(|| a.rule.cmp(&b.rule)));
    Ok(rows)
}
