//! How much of a window a repository's main was green: the lander's own after-landing verdicts (a red episode runs
//! from its first red verdict to the green one that ends it) and GitHub's Cargo runs on main (red from a failing run's
//! finish to the next passing run's finish). The reading is the share of the window not inside a red episode, and its
//! sample counts the red episodes that touched the window.
use std::collections::HashMap;

use rusqlite::Connection;

use super::{Reading, Window, state_root};
use crate::foundation::clock::Timestamp;
use crate::platform::watch_named;

const UNJUDGED: [&str; 3] = ["cancelled", "skipped", "neutral"];
const WORKFLOW: &str = "cargo.yml";

pub(super) type Reds = Vec<(i64, i64)>;

pub(super) fn lander(db: &Connection, now: i64) -> Result<HashMap<String, Reds>, String> {
    let Ok(mut statement) = db.prepare("SELECT repo, red_at, coalesce(green_at, '') FROM main_red") else { return Ok(HashMap::new()) };
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(|why| format!("main_red: {why}"))?;
    let mut found: HashMap<String, Reds> = HashMap::new();
    for (repo, red, green) in rows.filter_map(Result::ok) {
        let Ok(red) = Timestamp::parse(&red) else { continue };
        let end = Timestamp::parse(&green).map_or(now, Timestamp::unix_seconds);
        found.entry(repo).or_default().push((red.unix_seconds(), end));
    }
    Ok(found)
}

/// The red episodes of the repository's Cargo runs on main, or None when its runs were never recorded here.
pub(super) fn github(repo: &str, now: i64) -> Option<Reds> {
    let text = std::fs::read_to_string(state_root().join(format!("north/watch/runs-{repo}.json"))).ok()?;
    episodes(&text, now)
}

fn episodes(runs: &str, now: i64) -> Option<Reds> {
    let body: serde_json::Value = serde_json::from_str(runs).ok()?;
    let mut judged: Vec<(String, i64, bool)> = body["workflow_runs"]
        .as_array()?
        .iter()
        .filter(|run| run["path"].as_str().is_some_and(|path| path.ends_with(WORKFLOW)) && run["head_branch"] == "main" && run["event"] == "push" && run["status"] == "completed")
        .filter_map(|run| {
            let conclusion = run["conclusion"].as_str().filter(|conclusion| !UNJUDGED.contains(conclusion))?;
            Some((run["created_at"].as_str()?.to_owned(), i64::try_from(watch_named::run_time(run["updated_at"].as_str()?)?).ok()?, conclusion == "success"))
        })
        .collect();
    judged.sort();
    let mut reds: Reds = Vec::new();
    let mut open = false;
    for (_, finished, success) in judged {
        match (success, open) {
            (true, true) => {
                if let Some(last) = reds.last_mut() {
                    last.1 = finished;
                }
                open = false;
            }
            (false, false) => {
                reds.push((finished, now));
                open = true;
            }
            _ => {}
        }
    }
    Some(reds)
}

pub(super) fn reading(reds: &[(i64, i64)], window: Window) -> Reading {
    let length = (window.end - window.start).max(1);
    let touching: Vec<&(i64, i64)> = reds.iter().filter(|&&(from, to)| to >= window.start && from < window.end).collect();
    let red: i64 = touching.iter().map(|&&(from, to)| (to.min(window.end) - from.max(window.start)).max(0)).sum();
    Reading { value: (1.0 - red as f64 / length as f64).clamp(0.0, 1.0) * 100.0, sample: touching.len() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(created: &str, updated: &str, conclusion: &str) -> String {
        format!(r#"{{"path":".github/workflows/cargo.yml","head_branch":"main","event":"push","status":"completed","conclusion":"{conclusion}","created_at":"{created}","updated_at":"{updated}"}}"#)
    }

    #[test]
    fn a_red_episode_runs_from_the_first_failing_run_to_the_next_passing_one_and_the_share_green_is_the_rest_of_the_window() {
        let runs = format!(
            r#"{{"workflow_runs":[{},{},{},{}]}}"#,
            run("2026-11-08T01:00:00Z", "2026-11-08T01:10:00Z", "success"),
            run("2026-11-08T02:00:00Z", "2026-11-08T02:10:00Z", "failure"),
            run("2026-11-08T03:00:00Z", "2026-11-08T03:10:00Z", "failure"),
            run("2026-11-08T04:00:00Z", "2026-11-08T04:10:00Z", "success"),
        );
        let day = i64::try_from(watch_named::run_time("2026-11-08T00:00:00Z").unwrap()).unwrap();
        let now = day + 5 * 3_600;
        let reds = episodes(&runs, now).unwrap();
        assert_eq!(reds, vec![(day + 2 * 3_600 + 600, day + 4 * 3_600 + 600)]);
        let reading = reading(&reds, Window { start: day, end: day + 5 * 3_600 });
        assert_eq!(reading.sample, 1);
        assert!((reading.value - 60.0).abs() < 1e-9, "{}", reading.value);
    }
}
