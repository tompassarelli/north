use std::collections::BTreeMap;
use std::path::Path;

use super::Gc;
use super::evidence::{LaneClass, cited_issues};
use super::exec::git;
use super::state::Owned;
use north_foundation::foundation::clock::Timestamp;

const DAY_MS: i64 = 86_400_000;
const ALERT_ROWS: usize = 12;

pub(super) struct Row<'a> {
    pub(super) place: &'a str,
    pub(super) branch: &'a str,
    pub(super) ahead: u64,
    pub(super) dirty: &'a str,
    pub(super) class: LaneClass,
}

fn leading_hours(row: &str) -> i64 {
    let digits = row.bytes().enumerate().take_while(|(i, b)| b.is_ascii_digit() || (*i == 0 && *b == b'-')).count();
    row[..digits].parse().unwrap_or(0)
}

const CLASSES: [&str; 2] = ["landed-by-tree", "lost"];

pub(super) fn class_summary(rows: &[String]) -> Vec<String> {
    let mut repos: BTreeMap<&str, [usize; 2]> = BTreeMap::new();
    for row in rows.iter().filter(|row| !row.is_empty() && !row.starts_with('#')) {
        let fields: Vec<&str> = row.split('\t').collect();
        let class = fields.get(8).and_then(|label| CLASSES.iter().position(|known| known == label)).unwrap_or(1);
        repos.entry(fields.get(1).copied().unwrap_or("-")).or_default()[class] += 1;
    }
    repos
        .into_iter()
        .map(|(repo, counts)| {
            let cells: Vec<String> = CLASSES.iter().zip(counts).map(|(class, count)| format!("{class}={count}")).collect();
            format!("# classes {repo}: {}", cells.join(" "))
        })
        .collect()
}

pub(super) fn sorted(rows: &[String]) -> Vec<String> {
    let mut rows = rows.to_vec();
    rows.sort_by_key(|row| std::cmp::Reverse(leading_hours(row)));
    rows
}

impl Gc {
    pub(super) fn unlanded_row(&mut self, name: &str, main: &Path, base: &str, tip: &str, when: i64, row: &Row) -> i64 {
        let hours = (self.now_ms - when * 1000).div_euclid(3_600_000);
        let range = format!("{base}..{tip}");
        let commits = git(main, &["log", "--format=%s%x1f%B%x1e", &range]).out;
        let commits: Vec<(&str, &str)> = commits.split_terminator('\x1e').map(|commit| commit.trim_start_matches('\n').split_once('\x1f').unwrap_or((commit, ""))).collect();
        let first = commits.iter().rev().map(|(subject, _)| *subject).find(|subject| !subject.is_empty()).unwrap_or(if row.dirty.is_empty() { "-" } else { "uncommitted changes" });
        let bodies: String = commits.iter().map(|(_, body)| format!("{body}\n")).collect();
        let refs = cited_issues(&bodies).join(" ");
        let fields = [
            format!("{hours}h"),
            name.to_string(),
            row.place.to_string(),
            if row.branch.is_empty() { "(detached)".into() } else { row.branch.to_string() },
            format!("+{}", row.ahead),
            if row.dirty.is_empty() { "-".into() } else { row.dirty.to_string() },
            first.to_string(),
            if refs.is_empty() { "-".into() } else { refs },
            row.class.report_label().to_string(),
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
        for (parent, mut rows) in parents {
            let Some(age) = rows.iter().map(|row| row.age_h).max() else {
                continue;
            };
            if rows.len() <= 25 && age <= 72 {
                continue;
            }
            rows.sort_by_key(|row| std::cmp::Reverse(row.age_h));
            let shown: Vec<String> = rows.iter().take(ALERT_ROWS).map(|row| format!("{} {} {}h {}", row.place, row.branch, row.age_h, row.refs.join(" "))).collect();
            let more = rows.len().saturating_sub(ALERT_ROWS);
            let text = format!(
                "{parent}: {} lanes hold unique work, oldest first (repository branch age issues): {}{}; land, continue or archive each (north lane gc --unlanded)",
                rows.len(),
                shown.join("; "),
                if more > 0 { format!("; +{more} more") } else { String::new() }
            );
            if self.options.dry_run {
                println!("would alert {text}");
                continue;
            }
            let alerts = self.stale.alerts.get_or_insert_default();
            if alerts.get(&parent).and_then(|at| Timestamp::parse_rfc3339(at)).is_some_and(|at| self.now_ms - at.unix_millis() < DAY_MS) {
                continue;
            }
            alerts.insert(parent, Timestamp::from_unix_millis(self.now_ms).iso_millis());
            println!("lane gc: alerted {text}");
        }
    }
}

impl Gc {
    pub(super) fn print_outcome(&self) {
        let c = &self.counts;
        println!(
            "lane gc{}: retired {} worktrees, {} local branches, {} origin branches; bundled {} lanes to the archive before reaping them; kept {} ({} unlanded); dropped {} build targets ({}); pruned {} branch configs; dropped {} lane upstreams",
            if self.options.dry_run { " (dry run)" } else { "" },
            c.retired_worktrees,
            c.deleted_branches,
            c.deleted_remote,
            c.archived,
            c.kept,
            c.unlanded,
            c.dropped_targets,
            super::targets::gib(c.dropped_bytes),
            c.pruned_branch_configs,
            c.dropped_upstreams
        );
        println!("{}: archived {} lanes, freed {:.1} GiB, refused {}", north_base::platform::host::name(), c.archived, c.archived_bytes as f64 / f64::from(1u32 << 30), c.refused);
    }
}
