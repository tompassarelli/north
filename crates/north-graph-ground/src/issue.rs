use north_graph::Term;

use crate::reach::Reach;
use crate::show::{About, corrections, cut, day, short};

const OPEN_SHOWN: usize = 2;
const DONE_SHOWN: usize = 3;
const LANDINGS_SHOWN: usize = 3;

fn box_text(text: &str) -> String {
    cut(text.split(" (owner").next().unwrap_or(text), 60)
}

pub fn lines(reach: &Reach, issue: &str) -> Vec<String> {
    let about = About(reach.about(&Term::string(issue)));
    let Some(title) = about.first_text(issue, "title") else { return Vec::new() };
    let state = about.of(issue, "state").find_map(|term| match term {
        Term::Atom(north_graph::Atom::Keyword(state)) => Some(state.as_str()),
        _ => None,
    });
    let labels: Vec<&str> = about.texts(issue, "label").collect();
    let blockers: Vec<&str> = about.texts(issue, "blocked-by").collect();
    let mut head = format!("Graph {issue} \"{}\" ({}", cut(title, 80), state.unwrap_or("state unknown"));
    if !labels.is_empty() {
        head += &format!("; labels {}", labels.join(", "));
    }
    if !blockers.is_empty() {
        head += &format!("; blocked by {}", blockers.join(", "));
    }
    let mut lines = vec![head + ")"];
    let mut open: Vec<String> = Vec::new();
    let mut done: Vec<(bool, String)> = Vec::new();
    let mut commits: Vec<String> = about.texts(issue, "landed-as").map(str::to_owned).collect();
    for node in about.texts(issue, "has-box") {
        if reach.late() {
            break;
        }
        let tell = About(reach.about(&Term::string(node)));
        let text = box_text(tell.first_text(node, "text").unwrap_or_default());
        let ticked = tell.of(node, "ticked").any(|term| *term == Term::boolean(true));
        let owner = tell.first_text(node, "owner");
        if !ticked {
            open.push(format!("  open box: {text}{}", owner.map_or(String::new(), |owner| format!(" (owner {owner})"))));
            continue;
        }
        for (who, commit) in tell.claims(node) {
            let (observed, verdict) = match commit {
                None => (false, "claims done and names no commit: a claim, nothing observed".to_owned()),
                Some(commit) => {
                    commits.push(commit.to_owned());
                    let seen = About(reach.about(&Term::string(commit)));
                    match seen.observed(commit) {
                        Some((true, command)) => (true, format!("claims done at {}: observed, checker passed `{}`", short(commit), cut(command, 40))),
                        Some((false, command)) => (false, format!("claims done at {}: checker saw `{}` fail", short(commit), cut(command, 40))),
                        None => (false, format!("claims done at {}: a claim, no checker observation", short(commit))),
                    }
                }
            };
            done.push((observed, format!("  done box: {text} — {who} {verdict}")));
        }
    }
    done.sort_by_key(|(observed, _)| *observed);
    lines.extend(open.into_iter().take(OPEN_SHOWN));
    lines.extend(done.into_iter().take(DONE_SHOWN).map(|(_, line)| line));
    lines.extend(landings(reach, &mut commits));
    let sessions: Vec<&str> = about.subjects("works-on", issue).collect();
    lines.extend(corrections(reach, &sessions));
    lines
}

fn landings(reach: &Reach, commits: &mut Vec<String>) -> Option<String> {
    commits.sort_unstable();
    commits.dedup();
    let mut landed: Vec<(i64, String)> = Vec::new();
    for commit in commits.iter() {
        if reach.late() {
            break;
        }
        let about = About(reach.about(&Term::string(commit.as_str())));
        if let Some(at) = about.when(commit, "landed-at") {
            let by = about.first_text(commit, "landed-by").map_or(String::new(), |session| format!(" by {session}"));
            landed.push((at, format!("{}{by} on {}", short(commit), day(at))));
        }
    }
    landed.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let shown: Vec<String> = landed.into_iter().take(LANDINGS_SHOWN).map(|(_, line)| line).collect();
    (!shown.is_empty()).then(|| format!("  recent landings: {}", shown.join("; ")))
}
