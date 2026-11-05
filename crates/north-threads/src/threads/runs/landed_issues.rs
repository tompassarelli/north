//! `north threads landed-issues`: of the issues a set of fleet sessions landed, the ones a run of those
//! sessions reported done and landed.
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::super::verb::{Args, Context, Failure, Outcome, open};

static TICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:^|[^a-z])ticked\b|\[x\]").unwrap());
static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:unticked|(?:not|n't|never|no)\b[^.;\n]{0,30}\bticked)\b").unwrap());

/// One commit a fleet session put on main under an issue, with every conversation that session ran as.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Landing {
    pub issue: String,
    pub session: String,
    pub sha: String,
    pub conversations: Vec<String>,
}

/// A run counts as done and landed when it landed and reported done, or landed and reported `Not done:` with
/// its own box ticked: a box worker's report judges the issue, which stays open while other boxes remain.
pub fn done_and_landed(outcome: &str, report: &str, landed: bool) -> bool {
    landed && (outcome == "done" || (outcome == "not done" && TICKED.is_match(report) && !UNTICKED.is_match(&report.to_lowercase().replace("not done", ""))))
}

pub(super) fn landed_issues(argv: &[String], context: &Context, stdin: &mut dyn Read) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    if !args.positional.is_empty() {
        return Err(Failure::Usage("usage: threads landed-issues < landings.json".into()));
    }
    let landings: Vec<Landing> = serde_json::from_reader(stdin).map_err(|error| Failure::Usage(format!("threads landed-issues reads a JSON array of landings on stdin: {error}")))?;
    let db = open(context.database, false)?;
    let mut statement = db.prepare("SELECT outcome, coalesce(report, ''), (coalesce(landed, 0) = 1 OR landed_sha IS NOT NULL) FROM runs WHERE agent = ?1")?;
    let mut issues: BTreeMap<&str, (Vec<&Landing>, bool)> = BTreeMap::new();
    for landing in &landings {
        let mut covered = false;
        for conversation in &landing.conversations {
            let runs = statement.query_map(params![conversation], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?;
            for run in runs {
                let (outcome, report, landed) = run?;
                covered |= done_and_landed(&outcome, &report, landed);
            }
        }
        let entry = issues.entry(&landing.issue).or_default();
        entry.0.push(landing);
        entry.1 |= covered;
    }
    let mut shown = String::new();
    for (issue, (landed, covered)) in &issues {
        let sessions: Vec<String> = landed.iter().map(|landing| format!("{} {}", landing.session, short(&landing.sha))).collect();
        shown += &format!("{} {issue} {}\n", if *covered { "done" } else { "miss" }, sessions.join(", "));
    }
    let covered = issues.values().filter(|(_, covered)| *covered).count();
    let share = if issues.is_empty() { 0 } else { covered * 100 / issues.len() };
    shown += &format!("{covered} of {} landed issues have a done-and-landed run ({share}%)\n", issues.len());
    Ok(shown)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_report_counts_when_it_landed_and_ticked_its_box_and_not_when_it_only_landed() {
        assert!(done_and_landed("done", "Done: landed as 1234abcd.", true));
        assert!(done_and_landed("not done", "Not done: box 2 landed as 4d49abbc3 and is ticked; boxes 3-4 remain open.", true));
        assert!(done_and_landed("not done", "Not done: the issue stays open.\n- [x] settle the transient bound (4d49abbc3)", true));
        assert!(!done_and_landed("not done", "Not done: landed as 4d49abbc3, but the box is not ticked: its check still fails.", true));
        assert!(!done_and_landed("not done", "Not done: box 2 stays unticked.", true));
        assert!(!done_and_landed("not done", "Not done: the change is queued.", true));
        assert!(!done_and_landed("done", "Done: box ticked.", false));
        assert!(!done_and_landed("unclear", "Box ticked.", true));
    }
}
