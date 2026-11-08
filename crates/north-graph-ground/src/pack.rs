use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use north_graph::{Graph, GraphError, Term, default_path};

use crate::file::tests;
use crate::reach::Reach;
use crate::show::{About, corrections, cut, short};

const PATIENCE: Duration = Duration::from_millis(40);
const COMMITS_READ: usize = 40;
const COCHANGED_SHOWN: usize = 4;
const SESSIONS_READ: usize = 6;
const ISSUES_READ: usize = 3;
const CLAIMS_SHOWN: usize = 3;

/// What the fact graph holds on `path` for a brief's context pack: the tests that reach it, the files
/// its commits also touched, the claims on the issues of the sessions that landed them (and on `issues`)
/// that nothing observed, and the corrections those sessions recorded. A graph that is missing, locked
/// or too slow before `until` costs the lines, never an error.
pub fn lines(path: &str, issues: &[String], until: Instant) -> Vec<String> {
    lines_from(&default_path(), path, issues, until)
}

pub(crate) fn lines_from(db: &std::path::Path, path: &str, issues: &[String], until: Instant) -> Vec<String> {
    match Graph::open_read_only(db, PATIENCE) {
        Ok(graph) => read(&Reach::new(&graph, Some(until)), path, issues),
        Err(GraphError::Storage(_)) => vec!["graph: not read, no readable graph.db on this host; its claims and corrections are unavailable".to_owned()],
        Err(why) => vec![format!("graph: not read ({why}); its claims and corrections are unavailable")],
    }
}

fn read(reach: &Reach, path: &str, issues: &[String]) -> Vec<String> {
    let about = About(reach.about(&Term::string(path)));
    let functions: Vec<&str> = about.subjects("in-file", path).collect();
    let touched: Vec<&str> = about.subjects("touched", path).take(COMMITS_READ).collect();
    if functions.is_empty() && touched.is_empty() {
        return vec!["graph: holds nothing on it yet; no claims or corrections recorded".to_owned()];
    }
    let mut lines: Vec<String> = tests(reach, &functions).map(|line| line.trim_start().to_owned()).into_iter().collect();
    let mut together: BTreeMap<String, usize> = BTreeMap::new();
    let mut landed: Vec<(i64, String)> = Vec::new();
    for commit in &touched {
        if reach.late() {
            break;
        }
        let seen = About(reach.about(&Term::string(*commit)));
        for file in seen.texts(commit, "touched").filter(|file| *file != path) {
            *together.entry(file.to_owned()).or_default() += 1;
        }
        if let (Some(at), Some(by)) = (seen.when(commit, "landed-at"), seen.first_text(commit, "landed-by")) {
            landed.push((at, by.to_owned()));
        }
    }
    let mut files: Vec<(String, usize)> = together.into_iter().collect();
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !files.is_empty() {
        let named: Vec<String> = files.into_iter().take(COCHANGED_SHOWN).map(|(file, count)| format!("{file} ({count})")).collect();
        lines.push(format!("touched in the same commits: {}", named.join(", ")));
    }
    landed.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let mut sessions: Vec<&str> = Vec::new();
    for (_, session) in &landed {
        if !sessions.contains(&session.as_str()) {
            sessions.push(session);
        }
    }
    sessions.truncate(SESSIONS_READ);
    let mut worked: Vec<String> = issues.to_vec();
    for session in &sessions {
        for issue in About(reach.about(&Term::string(*session))).texts(session, "works-on") {
            if !worked.iter().any(|known| known == issue) {
                worked.push(issue.to_owned());
            }
        }
    }
    let claims: Vec<String> = worked.iter().take(ISSUES_READ).flat_map(|issue| unverified(reach, issue)).take(CLAIMS_SHOWN).collect();
    lines.push(if claims.is_empty() { "claims: no unverified claim on its issues".to_owned() } else { format!("claims, quoted and unverified (no checker or lander assertion): {}", claims.join("; ")) });
    lines.push(corrections(reach, &sessions).map_or_else(|| "corrections: none recorded in the sessions that touched it".to_owned(), |line| line.trim_start().to_owned()));
    lines
}

fn unverified(reach: &Reach, issue: &str) -> Vec<String> {
    let about = About(reach.about(&Term::string(issue)));
    let mut found = Vec::new();
    for node in about.texts(issue, "has-box") {
        if reach.late() {
            break;
        }
        let tell = About(reach.about(&Term::string(node)));
        if !tell.of(node, "ticked").any(|term| *term == Term::boolean(true)) {
            continue;
        }
        let text = cut(tell.first_text(node, "text").unwrap_or_default().split(" (owner").next().unwrap_or_default(), 50);
        for (who, commit) in tell.claims(node) {
            let said = match commit {
                None => format!("{issue} \"{text}\": {who} claims it done, names no commit"),
                Some(commit) => {
                    let seen = About(reach.about(&Term::string(commit)));
                    if seen.observed(commit).is_some_and(|(passed, _)| passed) || seen.when(commit, "landed-at").is_some() {
                        continue;
                    }
                    format!("{issue} \"{text}\": {who} claims it done at {}", short(commit))
                }
            };
            found.push(said);
        }
    }
    found
}
