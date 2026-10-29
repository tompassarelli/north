use std::collections::BTreeMap;
use std::path::Path;

use super::Gc;
use super::evidence::cited_issues;
use super::exec::{command, git};
use super::state::Owned;
use crate::foundation::clock::Timestamp;

const DAY_MS: i64 = 86_400_000;

pub(super) struct Row<'a> {
    pub(super) place: &'a str,
    pub(super) branch: &'a str,
    pub(super) ahead: u64,
    pub(super) dirty: &'a str,
}

fn leading_hours(row: &str) -> i64 {
    let digits = row.bytes().enumerate().take_while(|(i, b)| b.is_ascii_digit() || (*i == 0 && *b == b'-')).count();
    row[..digits].parse().unwrap_or(0)
}

pub(super) fn sorted(rows: &[String]) -> Vec<String> {
    let mut rows = rows.to_vec();
    rows.sort_by_key(|row| std::cmp::Reverse(leading_hours(row)));
    rows
}

impl Gc {
    pub(super) fn unlanded_row(&mut self, name: &str, main: &Path, base: &str, tip: &str, row: &Row) -> i64 {
        let when: i64 = git(main, &["log", "-1", "--format=%ct", tip]).out.parse().unwrap_or(0);
        let hours = (self.now_ms - when * 1000).div_euclid(3_600_000);
        let range = format!("{base}..{tip}");
        let subjects = git(main, &["log", "--reverse", "--format=%s", &range]).out;
        let first = subjects.lines().find(|line| !line.is_empty()).unwrap_or(if row.dirty.is_empty() { "-" } else { "uncommitted changes" });
        let refs = cited_issues(&git(main, &["log", "--format=%B", &range]).out).join(" ");
        let fields = [
            format!("{hours}h"),
            name.to_string(),
            row.place.to_string(),
            if row.branch.is_empty() { "(detached)".into() } else { row.branch.to_string() },
            format!("+{}", row.ahead),
            if row.dirty.is_empty() { "-".into() } else { row.dirty.to_string() },
            first.to_string(),
            if refs.is_empty() { "-".into() } else { refs },
        ];
        self.rows.push(fields.join("\t"));
        self.counts.unlanded += 1;
        hours
    }

    pub(super) fn raise_alerts(&mut self) {
        let mut parents: BTreeMap<String, Vec<Owned>> = BTreeMap::new();
        for (repo, report) in &self.stale.repos {
            for row in report.owned.iter().flatten() {
                parents.entry(row.owner.clone()).or_default().push(Owned { place: repo.clone(), ..row.clone() });
            }
        }
        for (parent, rows) in parents {
            let Some(age) = rows.iter().map(|row| row.age_h).max() else {
                continue;
            };
            let Some(oldest) = rows.iter().find(|row| row.age_h == age).filter(|_| rows.len() > 25 || age > 72) else {
                continue;
            };
            let text = format!(
                "{parent}: {} owned unlanded lanes, oldest {}h ({} {}); continue, land or archive each (north lane-gc --unlanded)",
                rows.len(),
                oldest.age_h,
                oldest.place,
                oldest.branch
            );
            if self.options.dry_run {
                println!("would alert {text}");
                continue;
            }
            let alerts = self.stale.alerts.get_or_insert_default();
            if alerts.get(&parent).and_then(|at| Timestamp::parse_rfc3339(at)).is_some_and(|at| self.now_ms - at.unix_millis() < DAY_MS) {
                continue;
            }
            if !command(&["supply-alert", "normal", &text, "Unlanded lanes"]).ok {
                println!("lane-gc: supply-alert failed for {parent}");
                continue;
            }
            alerts.insert(parent, Timestamp::from_unix_millis(self.now_ms).iso_millis());
            println!("lane-gc: alerted {text}");
        }
    }
}
