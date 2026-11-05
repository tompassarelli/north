use std::collections::HashMap;

use super::evidence::{IssueState, item_ref};

const HOUR_MS: i64 = 3_600_000;
pub(super) const STUCK_MS: i64 = 24 * HOUR_MS;
const GRACE_MS: i64 = HOUR_MS;
pub(super) const STALE_STATE_MS: i64 = 26 * HOUR_MS;

pub(super) struct Facts<'a> {
    pub(super) repo: &'a str,
    pub(super) branch: &'a str,
    pub(super) committed_ms: i64,
    pub(super) landed: bool,
    pub(super) session: Option<String>,
    pub(super) ticket: bool,
    pub(super) issues: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) repo: String,
    pub(super) branch: String,
    pub(super) age_ms: i64,
    pub(super) owner: String,
    pub(super) issues: Vec<String>,
    pub(super) leftover: bool,
    pub(super) alarm: bool,
    pub(super) owned: bool,
}

pub(super) fn row(facts: &Facts, now: i64) -> Row {
    let owner = match (&facts.session, facts.ticket) {
        (Some(session), _) => format!("session {session}"),
        (None, true) => "land ticket".to_string(),
        (None, false) => "none".to_string(),
    };
    let issues = facts.issues.iter().map(|item| if item.starts_with('#') { format!("{}{item}", facts.repo) } else { item_ref(item) }).collect();
    let age_ms = (now - facts.committed_ms).max(0);
    let leftover = facts.landed && facts.session.is_none() && !facts.ticket;
    Row { repo: facts.repo.to_string(), branch: facts.branch.to_string(), age_ms, owner, issues, leftover, alarm: leftover && age_ms > GRACE_MS, owned: facts.session.is_some() || facts.ticket }
}

pub(super) fn age_text(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match (minutes / 1440, minutes / 60 % 24) {
        (0, 0) => format!("{minutes}m"),
        (0, hours) => format!("{hours}h {}m", minutes % 60),
        (days, hours) => format!("{days}d {hours}h"),
    }
}

fn grid(header: &[&str], lines: Vec<Vec<String>>) -> Vec<String> {
    let mut cells: Vec<Vec<String>> = vec![header.iter().map(|name| name.to_string()).collect()];
    cells.extend(lines);
    let widths: Vec<usize> = (0..header.len()).map(|column| cells.iter().map(|line| line[column].chars().count()).max().unwrap_or(0)).collect();
    cells.iter().map(|line| line.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end().to_string()).collect()
}

fn oldest_first(rows: &[Row], leftover: bool) -> Vec<&Row> {
    let mut chosen: Vec<&Row> = rows.iter().filter(|row| row.leftover == leftover).collect();
    chosen.sort_by(|a, b| b.age_ms.cmp(&a.age_ms).then_with(|| a.repo.cmp(&b.repo)).then_with(|| a.branch.cmp(&b.branch)));
    chosen
}

pub(super) fn leftover_table(rows: &[Row]) -> Vec<String> {
    let lines = oldest_first(rows, true).into_iter().map(|row| vec![row.repo.clone(), row.branch.clone(), age_text(row.age_ms), row.owner.clone()]).collect();
    grid(&["REPO", "BRANCH", "AGE", "OWNER"], lines)
}

pub(super) fn fault(row: &Row, states: &HashMap<String, IssueState>) -> Option<&'static str> {
    if row.alarm {
        return Some("landed, still present");
    }
    if row.leftover || row.owned {
        return None;
    }
    if row.issues.is_empty() {
        return Some("names no issue");
    }
    let closed = |issue: &String| states.get(issue) == Some(&IssueState::Closed);
    row.issues.iter().all(closed).then_some("all its issues are closed")
}

fn state_text(issue: &str, states: &HashMap<String, IssueState>) -> &'static str {
    match states.get(issue) {
        Some(IssueState::Open) => "open",
        Some(IssueState::Closed) => "closed",
        _ => "unknown",
    }
}

pub(super) fn open_table(rows: &[Row], states: &HashMap<String, IssueState>) -> Vec<String> {
    let mut groups: Vec<(String, Vec<&Row>)> = Vec::new();
    for row in oldest_first(rows, false) {
        let keys: Vec<String> = if row.issues.is_empty() { vec!["(no issue)".to_string()] } else { row.issues.clone() };
        for key in keys {
            match groups.iter_mut().find(|(name, _)| *name == key) {
                Some((_, members)) => members.push(row),
                None => groups.push((key, vec![row])),
            }
        }
    }
    groups.sort_by(|a, b| b.1[0].age_ms.cmp(&a.1[0].age_ms).then_with(|| a.0.cmp(&b.0)));
    let mut lines = Vec::new();
    for (issue, members) in &groups {
        for (index, row) in members.iter().enumerate() {
            let (name, state) = if index == 0 { (issue.clone(), if issue.contains('#') { state_text(issue, states).to_string() } else { "-".to_string() }) } else { (String::new(), String::new()) };
            lines.push(vec![name, state, row.repo.clone(), row.branch.clone(), age_text(row.age_ms), row.owner.clone(), fault(row, states).unwrap_or("").to_string()]);
        }
    }
    grid(&["ISSUE", "STATE", "REPO", "BRANCH", "AGE", "OWNER", "FAULT"], lines)
}

pub(super) fn gc_problems(unit_results: &[(&str, String)], state_age_ms: Option<i64>, rows: &[Row]) -> Vec<String> {
    let mut problems: Vec<String> = unit_results.iter().filter(|(_, result)| result != "success").map(|(unit, result)| format!("{unit} last result: {result}")).collect();
    match state_age_ms {
        None => problems.push("lane-gc has no state file".into()),
        Some(age) if age > STALE_STATE_MS => problems.push(format!("lane-gc has not run for {}", age_text(age))),
        Some(_) => {}
    }
    let stuck = rows.iter().filter(|row| row.leftover && row.age_ms > STUCK_MS).count();
    if stuck > 0 {
        problems.push(format!("lane-gc retired nothing for a day while {stuck} landed branches still exist"));
    }
    problems
}
