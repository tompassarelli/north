use north_graph::Term;

use crate::reach::Reach;
use crate::show::{About, corrections, day, short};

const FUNCTIONS_READ: usize = 30;
const TESTS_SHOWN: usize = 3;
const COMMITS_READ: usize = 80;
const LANDINGS_SHOWN: usize = 3;

pub fn lines(reach: &Reach, path: &str) -> Vec<String> {
    let about = About(reach.about(&Term::string(path)));
    let functions: Vec<&str> = about.subjects("in-file", path).collect();
    let touched: Vec<&str> = about.subjects("touched", path).collect();
    if functions.is_empty() && touched.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!("Graph {path}")];
    lines.extend(tests(reach, &functions));
    let mut landed: Vec<(i64, String, Option<String>)> = Vec::new();
    for commit in touched.iter().take(COMMITS_READ) {
        if reach.late() {
            break;
        }
        let seen = About(reach.about(&Term::string(*commit)));
        if let Some(at) = seen.when(commit, "landed-at") {
            landed.push((at, (*commit).to_owned(), seen.first_text(commit, "landed-by").map(str::to_owned)));
        }
    }
    landed.sort_by_key(|(at, ..)| std::cmp::Reverse(*at));
    landed.truncate(LANDINGS_SHOWN);
    let mut sessions: Vec<&str> = Vec::new();
    let mut shown = Vec::new();
    for (at, commit, by) in &landed {
        let issues = by.as_deref().map(|session| About(reach.about(&Term::string(session))).texts(session, "works-on").map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();
        let who = by.as_deref().map_or(String::new(), |session| format!(" by {session}"));
        let work = if issues.is_empty() { String::new() } else { format!(" for {}", issues.join(", ")) };
        shown.push(format!("{}{who}{work} on {}", short(commit), day(*at)));
        sessions.extend(by.as_deref());
    }
    if !shown.is_empty() {
        lines.push(format!("  recent landings touching it: {}", shown.join("; ")));
    }
    lines.extend(corrections(reach, &sessions));
    lines
}

fn tests(reach: &Reach, functions: &[&str]) -> Option<String> {
    let mut tests: Vec<String> = Vec::new();
    for function in functions.iter().take(FUNCTIONS_READ) {
        if reach.late() {
            break;
        }
        for test in About(reach.about(&Term::string(*function))).subjects("reaches", function) {
            let name = test.strip_prefix("test:").unwrap_or(test).to_owned();
            if !tests.contains(&name) {
                tests.push(name);
            }
        }
    }
    if functions.is_empty() {
        return None;
    }
    let examples: Vec<&str> = tests.iter().take(TESTS_SHOWN).map(String::as_str).collect();
    Some(format!("  {} functions in the coverage map; {} tests reach them{}", functions.len(), tests.len(), if examples.is_empty() { String::new() } else { format!(", e.g. {}", examples.join(", ")) }))
}
