use north_github_mirror::store::fetch_open_issues;
use serde_json::Value;

use crate::platform::{cleanup, github};

const REPOSITORIES: [&str; 3] = ["north", "makgora", "wisp"];
const USAGE: &str = "usage: north cleanup-sweep [--dry-run] [north|makgora|wisp...]";

#[derive(Debug, Default, Eq, PartialEq)]
struct Sweep {
    pub open: usize,
    pub not_product: usize,
    pub already: usize,
    pub edits: Vec<(u64, String)>,
}

fn plan(issues: &[Value]) -> Sweep {
    let mut sweep = Sweep { open: issues.len(), ..Sweep::default() };
    for issue in issues {
        let labels: Vec<String> = issue["labels"]["nodes"].as_array().into_iter().flatten().filter_map(|label| label["name"].as_str().map(str::to_owned)).collect();
        let (Some(number), true) = (issue["number"].as_u64(), cleanup::applies(&labels)) else {
            sweep.not_product += 1;
            continue;
        };
        match cleanup::appended(issue["body"].as_str().unwrap_or_default()) {
            Some(body) => sweep.edits.push((number, body)),
            None => sweep.already += 1,
        }
    }
    sweep
}

fn edit(repo: &str, number: u64, body: &str) -> Result<(), String> {
    let file = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    std::fs::write(file.path(), body).map_err(|error| error.to_string())?;
    let args = ["issue", "edit", &number.to_string(), "-R", &format!("tompassarelli/{repo}"), "--body-file", &file.path().to_string_lossy()].map(str::to_owned);
    let output = github::run_owned(&args).map_err(|error| error.to_string())?;
    output.status.success().then_some(()).ok_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_owned())
}

pub fn run(arguments: &[String]) -> u8 {
    let dry = arguments.iter().any(|argument| argument == "--dry-run");
    let named: Vec<&str> = arguments.iter().map(String::as_str).filter(|argument| *argument != "--dry-run").collect();
    if named.iter().any(|repo| !REPOSITORIES.contains(repo)) {
        eprintln!("{USAGE}");
        return 2;
    }
    let mut failed = false;
    for repo in REPOSITORIES.iter().filter(|repo| named.is_empty() || named.contains(repo)) {
        let issues = match fetch_open_issues(repo) {
            Ok(issues) => issues,
            Err(why) => {
                eprintln!("{repo}: open issues unread: {why}");
                failed = true;
                continue;
            }
        };
        let sweep = plan(&issues);
        let mut added = 0;
        for (number, body) in &sweep.edits {
            match if dry { Ok(()) } else { edit(repo, *number, body) } {
                Ok(()) => added += 1,
                Err(why) => {
                    eprintln!("{repo}#{number}: not edited: {why}");
                    failed = true;
                }
            }
        }
        let verb = if dry { "would gain" } else { "gained" };
        println!("{repo}: {} open; {added} {verb} the Clean-up group; {} already carry it; {} are docs, research or roots and are skipped", sweep.open, sweep.already, sweep.not_product);
    }
    u8::from(failed)
}

