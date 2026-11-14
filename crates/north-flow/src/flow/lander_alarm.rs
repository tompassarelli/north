//! A stalled lander, told the way a red main is: while a repository's lander queue holds lanes and
//! origin/main stays unmoved past its limit, one urgent fleet message reaches the proxy and the
//! repository's "landing is stalled" issue opens; when the queue moves again one more message goes
//! out and the issue closes.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::host::{Host, Places, write_atomic};
use super::lander::Reading;
use crate::platform::rolling_issue::{Gh, settle};

pub const TITLE: &str = "landing is stalled";
const FILE: &str = "lander-stalls.json";
const FROM: &str = "service:north-ops-flow";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Episode {
    pub slug: String,
    pub text: String,
    pub told: bool,
    pub filed: bool,
}

pub type State = BTreeMap<String, Episode>;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Tell { repo: String, text: String },
    File { repo: String, slug: String, body: String },
    Recover { repo: String, text: String, told: bool, slug: Option<String> },
}

fn stalled_text(repo: &str, reading: &Reading) -> String {
    let oldest = reading.oldest_minutes.map_or_else(|| "unknown".to_owned(), |minutes| format!("{minutes:.0}m"));
    let line = reading.last_line.as_deref().unwrap_or("no lander log");
    format!(
        "Lander stalled in {repo}: {} lanes queued, the oldest waiting {oldest}, and origin/main unmoved {:.0}m > {:.0}m (its limit from the lander's p90 candidate check and floor); lander's last log line: {line}",
        reading.depth,
        reading.idle_minutes.unwrap_or_default(),
        reading.stall_limit_minutes,
    )
}

fn body(text: &str, repo: &str) -> String {
    format!("{text}\n\n## Done when\n- [ ] (headless run) origin/main moves through the lander again. Done when: judged by the lander alarm `north ledger flow --repo {repo}` raises, which closes this issue once it reads the lander moving.\n")
}

/// The steps due this run: a new stall tells and files, an unfinished stall retries what failed,
/// a cleared stall recovers. A row whose lander was not read leaves its episode as it was.
pub fn plan(rows: &[Value], previous: &State) -> (State, Vec<Step>) {
    let mut state = previous.clone();
    let mut steps = Vec::new();
    for row in rows {
        let Some(repo) = row["repo"].as_str() else { continue };
        let Ok(reading) = serde_json::from_value::<Reading>(row["lander"].clone()) else { continue };
        if !reading.stalled {
            if let Some(episode) = state.remove(repo) {
                let text = format!("Lander moving again in {repo}: origin/main moved or the queue drained ({} lanes queued now).", reading.depth);
                steps.push(Step::Recover { repo: repo.into(), text, told: episode.told, slug: episode.filed.then_some(episode.slug) });
            }
            continue;
        }
        let episode = state.entry(repo.to_owned()).or_insert_with(|| Episode { slug: row["slug"].as_str().unwrap_or_default().into(), text: stalled_text(repo, &reading), ..Episode::default() });
        if !episode.told {
            steps.push(Step::Tell { repo: repo.into(), text: episode.text.clone() });
        }
        if !episode.filed && !episode.slug.is_empty() {
            steps.push(Step::File { repo: repo.into(), slug: episode.slug.clone(), body: body(&episode.text, repo) });
        }
    }
    (state, steps)
}

fn load(path: &Path) -> State {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save(path: &Path, state: &State) -> Result<(), String> {
    write_atomic(path, &(serde_json::to_string(state).map_err(|error| error.to_string())? + "\n")).map_err(|error| error.to_string())
}

fn tell(host: &dyn Host, text: &str) -> bool {
    host.run(&["north", "fleet", "send", "--now", "--from", FROM, "proxy", text], None, None).is_some()
}

pub fn alert(places: &Places, host: &dyn Host, rows: &[Value]) -> Result<(), String> {
    let directory = places.flow();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(FILE);
    let (mut state, steps) = plan(rows, &load(&path));
    for step in steps {
        match step {
            Step::Tell { repo, text } => {
                let told = tell(host, &text);
                if !told {
                    println!("{repo}: lander stall message error");
                }
                state.entry(repo).and_modify(|episode| episode.told = told);
            }
            Step::File { repo, slug, body } => {
                let filed = settle(&mut Gh::of(&slug), TITLE, Some(&body), "");
                if let Err(error) = &filed {
                    println!("{repo}: lander stall issue error: {error}");
                }
                state.entry(repo).and_modify(|episode| episode.filed = filed.is_ok());
            }
            Step::Recover { repo, text, told, slug } => {
                if told && !tell(host, &text) {
                    println!("{repo}: lander recovery message error");
                }
                if let Some(slug) = slug
                    && let Err(error) = settle(&mut Gh::of(&slug), TITLE, None, &text)
                {
                    println!("{repo}: lander stall issue close error: {error}");
                }
            }
        }
        save(&path, &state)?;
    }
    save(&path, &state)
}

#[cfg(test)]
#[path = "lander_alarm_tests.rs"]
mod tests;
