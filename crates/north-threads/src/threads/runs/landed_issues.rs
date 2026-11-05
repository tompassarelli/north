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
    let mut issues: BTreeMap<&str, Vec<(&Landing, Option<String>)>> = BTreeMap::new();
    for landing in &landings {
        let mut runs = Vec::new();
        for conversation in &landing.conversations {
            let rows = statement.query_map(params![conversation], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?;
            for row in rows {
                runs.push(row?);
            }
        }
        let sessions = issues.entry(&landing.issue).or_default();
        if !sessions.iter().any(|(seen, _)| seen.session == landing.session && seen.sha == landing.sha) {
            sessions.push((landing, missed(&runs)));
        }
    }
    let mut shown = String::new();
    let mut covered = 0;
    for (issue, sessions) in &issues {
        let done = sessions.iter().any(|(_, why)| why.is_none());
        covered += usize::from(done);
        let listed: Vec<String> = sessions
            .iter()
            .map(|(landing, why)| match why.as_deref().filter(|_| !done) {
                Some(why) => format!("{} {} ({why})", landing.session, short(&landing.sha)),
                None => format!("{} {}", landing.session, short(&landing.sha)),
            })
            .collect();
        shown += &format!("{} {issue} {}\n", if done { "done" } else { "miss" }, listed.join(", "));
    }
    let share = if issues.is_empty() { 0 } else { covered * 100 / issues.len() };
    shown += &format!("{covered} of {} landed issues have a done-and-landed run ({share}%)\n", issues.len());
    Ok(shown)
}

/// Why a session's runs hold no done-and-landed run, or nothing when one of them is.
pub fn missed(runs: &[(String, String, bool)]) -> Option<String> {
    if runs.iter().any(|(outcome, report, landed)| done_and_landed(outcome, report, *landed)) {
        return None;
    }
    let outcomes = |landed: bool| {
        let mut named: Vec<&str> = runs.iter().filter(|run| run.2 == landed).map(|run| run.0.as_str()).collect();
        named.dedup();
        named.join(", ")
    };
    Some(if runs.is_empty() {
        "no run row".to_owned()
    } else if !runs.iter().any(|run| run.2) {
        format!("run landed=0, outcome {}", outcomes(false))
    } else if runs.iter().any(|(outcome, _, landed)| *landed && outcome == "not done") {
        "landed, Not done without its box ticked".to_owned()
    } else {
        format!("landed, outcome {}", outcomes(true))
    })
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

    #[test]
    fn a_miss_names_why_its_session_has_no_done_and_landed_run() {
        let run = |outcome: &str, report: &str, landed: bool| (outcome.to_owned(), report.to_owned(), landed);
        assert_eq!(missed(&[]), Some("no run row".into()));
        assert_eq!(missed(&[run("done", "Done: queued.", false)]), Some("run landed=0, outcome done".into()));
        assert_eq!(missed(&[run("not done", "Not done: box 3 remains.", true)]), Some("landed, Not done without its box ticked".into()));
        assert_eq!(missed(&[run("idle", "Noted.", true), run("idle", "Noted.", true)]), Some("landed, outcome idle".into()));
        assert_eq!(missed(&[run("idle", "Noted.", true), run("done", "Done: landed.", true)]), None);
    }
}
