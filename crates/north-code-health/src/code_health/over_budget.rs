use super::snapshot::git;
use north_base::foundation::clock::Timestamp;
use north_base::platform::rolling_issue::{Gh, Tracker, settle};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use toml_edit::DocumentMut;

const USAGE: &str = "usage: north code-health budget --json | north code-health over-budget [--repo DIR]";
const TITLE: &str = "North is over the Rust line budget";
const EXPERIMENTS: &str = "docs/rationale/experiments.toml";
const CANDIDATES: usize = 5;

#[derive(Deserialize)]
struct Measured {
    rust_lines: Option<Lines>,
}

#[derive(Deserialize)]
struct Lines {
    lines: usize,
    budget: usize,
}

struct Candidate {
    id: String,
    review: String,
    owner: String,
    change: String,
}

pub(super) fn run(arguments: &[String]) -> u8 {
    let repo = match arguments {
        [] => Path::new("."),
        [flag, dir] if flag == "--repo" => Path::new(dir),
        _ => return fail(USAGE),
    };
    let mut measured = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut measured) {
        return fail(&format!("cannot read the budget JSON on stdin: {error}"));
    }
    match settle_issue(&mut Gh::north(), repo, &measured, &Timestamp::now().iso_date()) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("north code-health over-budget: {message}");
    2
}

fn settle_issue(tracker: &mut dyn Tracker, repo: &Path, measured: &str, today: &str) -> Result<String, String> {
    let Measured { rust_lines } = serde_json::from_str(measured).map_err(|error| format!("stdin is not `north code-health budget --json` output: {error}"))?;
    let Lines { lines, budget } = rust_lines.ok_or("the budget JSON has no rust_lines; harness/defaults.toml names no [rust_line_budget]")?;
    let sha = git(repo, &["rev-parse", "--short=12", "HEAD"])?.trim().to_owned();
    if lines <= budget {
        let closing = format!("Main at {sha} counts {lines} non-test Rust lines, within the budget of {budget}, so this issue closes (north#1276).");
        settle(tracker, TITLE, None, &closing)?;
        return Ok(format!("{lines} non-test Rust lines at {sha}, within {budget}; no over-budget issue stays open"));
    }
    let body = body(&sha, lines, budget, &candidates(repo, today)?);
    settle(tracker, TITLE, Some(&body), "")?;
    Ok(format!("{lines} non-test Rust lines at {sha}, over {budget} by {}; the issue \"{TITLE}\" names the excess", lines - budget))
}

fn candidates(repo: &Path, today: &str) -> Result<Vec<Candidate>, String> {
    let text = match std::fs::read_to_string(repo.join(EXPERIMENTS)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read {EXPERIMENTS}: {error}")),
    };
    let document: DocumentMut = text.parse().map_err(|error| format!("{EXPERIMENTS} does not parse: {error}"))?;
    let field = |entry: &toml_edit::Table, key: &str| entry.get(key).and_then(|item| item.as_str()).unwrap_or_default().to_owned();
    let mut found: Vec<Candidate> = document
        .get("experiment")
        .and_then(|item| item.as_array_of_tables())
        .into_iter()
        .flatten()
        .map(|entry| Candidate {
            id: field(entry, "id"),
            review: field(entry, "review"),
            owner: field(entry, "owner"),
            change: [field(entry, "change"), field(entry, "box")].into_iter().find(|text| !text.is_empty()).unwrap_or_default(),
        })
        .filter(|candidate| !candidate.review.is_empty() && candidate.review.as_str() < today)
        .collect();
    found.sort_by(|a, b| a.review.cmp(&b.review).then_with(|| a.id.cmp(&b.id)));
    found.truncate(CANDIDATES);
    Ok(found)
}

fn body(sha: &str, lines: usize, budget: usize, candidates: &[Candidate]) -> String {
    let mut text = format!(
        "Main at {sha} counts {lines} non-test Rust lines against the budget of {budget} in harness/defaults.toml [rust_line_budget]: {} over (north#1276). `north code-health over-budget` keeps this one issue after each landing: it edits it while main stays over and closes it once a landing brings main back within the budget. No lane is refused for size (north#729).\n\n## Retirement candidates\n\n",
        lines - budget
    );
    if candidates.is_empty() {
        text.push_str(&format!("No entry in {EXPERIMENTS} is past its review date.\n"));
        return text;
    }
    text.push_str(&format!("The entries in {EXPERIMENTS} furthest past their review date, oldest first:\n\n"));
    for candidate in candidates {
        text.push_str(&format!("- `{}` (review {}, owner {}): {}\n", candidate.id, candidate.review, candidate.owner, candidate.change));
    }
    text
}
