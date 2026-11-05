const HOUR_MS: i64 = 3_600_000;
const IDLE_MS: i64 = 24 * HOUR_MS;
const LANDING_IDLE_MS: i64 = 5 * HOUR_MS;
pub(super) const STUCK_MS: i64 = 24 * HOUR_MS;
pub(super) const STALE_STATE_MS: i64 = 26 * HOUR_MS;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Cleanup {
    Now,
    WhenLanded,
    WhenOwnerEnds(String),
    At(i64),
}

pub(super) struct Facts<'a> {
    pub(super) repo: &'a str,
    pub(super) branch: &'a str,
    pub(super) committed_ms: i64,
    pub(super) landed: bool,
    pub(super) landing_ref: bool,
    pub(super) session: Option<String>,
    pub(super) ticket: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) repo: String,
    pub(super) branch: String,
    pub(super) age_ms: i64,
    pub(super) owner: String,
    pub(super) why: &'static str,
    pub(super) cleanup: Cleanup,
}

pub(super) fn row(facts: &Facts, now: i64) -> Row {
    let (owner, why, cleanup) = if let Some(session) = &facts.session {
        (format!("session {session}"), "running worker", Cleanup::WhenOwnerEnds(session.clone()))
    } else if facts.ticket {
        ("land ticket".to_string(), "queued lane", Cleanup::WhenLanded)
    } else if facts.landed {
        ("none".to_string(), "landed, not deleted", Cleanup::Now)
    } else {
        let idle = if facts.landing_ref { LANDING_IDLE_MS } else { IDLE_MS };
        ("none".to_string(), "unlanded, no owner", Cleanup::At(facts.committed_ms + idle))
    };
    Row { repo: facts.repo.to_string(), branch: facts.branch.to_string(), age_ms: (now - facts.committed_ms).max(0), owner, why, cleanup }
}

impl Row {
    pub(super) fn overdue_ms(&self, now: i64) -> Option<i64> {
        match self.cleanup {
            Cleanup::Now => Some(self.age_ms),
            Cleanup::At(at) if now >= at => Some(now - at),
            _ => None,
        }
    }
}

pub(super) fn age_text(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match (minutes / 1440, minutes / 60 % 24) {
        (0, 0) => format!("{minutes}m"),
        (0, hours) => format!("{hours}h {}m", minutes % 60),
        (days, hours) => format!("{days}d {hours}h"),
    }
}

fn cleanup_text(row: &Row, now: i64, when: &dyn Fn(i64) -> String) -> String {
    match &row.cleanup {
        Cleanup::Now => "now".into(),
        Cleanup::WhenLanded => "when the lane lands".into(),
        Cleanup::WhenOwnerEnds(owner) => format!("when {owner} ends"),
        Cleanup::At(at) if *at <= now => format!("overdue since {}", when(*at)),
        Cleanup::At(at) => when(*at),
    }
}

pub(super) fn table(rows: &[Row], now: i64, when: &dyn Fn(i64) -> String) -> Vec<String> {
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| b.age_ms.cmp(&a.age_ms).then_with(|| a.repo.cmp(&b.repo)).then_with(|| a.branch.cmp(&b.branch)));
    let mut cells = vec![["REPO", "BRANCH", "AGE", "OWNER", "WHY IT EXISTS", "CLEANED UP"].map(String::from)];
    cells.extend(sorted.iter().map(|row| [row.repo.clone(), row.branch.clone(), age_text(row.age_ms), row.owner.clone(), row.why.to_string(), cleanup_text(row, now, when)]));
    let widths: Vec<usize> = (0..6).map(|column| cells.iter().map(|line| line[column].chars().count()).max().unwrap_or(0)).collect();
    cells.iter().map(|line| line.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end().to_string()).collect()
}

pub(super) fn gc_problems(unit_results: &[(&str, String)], state_age_ms: Option<i64>, rows: &[Row], now: i64) -> Vec<String> {
    let mut problems: Vec<String> = unit_results.iter().filter(|(_, result)| result != "success").map(|(unit, result)| format!("{unit} last result: {result}")).collect();
    match state_age_ms {
        None => problems.push("lane-gc has no state file".into()),
        Some(age) if age > STALE_STATE_MS => problems.push(format!("lane-gc has not run for {}", age_text(age))),
        Some(_) => {}
    }
    let stuck = rows.iter().filter(|row| row.overdue_ms(now).is_some_and(|late| late > STUCK_MS)).count();
    if stuck > 0 {
        problems.push(format!("lane-gc retired nothing for a day while {stuck} branches are past their cleanup time"));
    }
    problems
}
