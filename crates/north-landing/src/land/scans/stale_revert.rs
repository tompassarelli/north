use std::path::Path;

use super::git;
use crate::landing::stale_revert;

const USAGE: &str = "usage: north land scan stale-revert [--repo DIR] --destination BRANCH --base OID [--lane TIP] HEAD";

#[derive(Default)]
struct Arguments {
    repo: String,
    destination: String,
    base: String,
    lane: String,
    head: String,
}

fn parse(arguments: &[String]) -> Option<Arguments> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--repo" => parsed.repo.clone_from(rest.next()?),
            "--destination" => parsed.destination.clone_from(rest.next()?),
            "--base" => parsed.base.clone_from(rest.next()?),
            "--lane" => parsed.lane.clone_from(rest.next()?),
            head if !head.starts_with('-') && parsed.head.is_empty() => parsed.head = head.to_owned(),
            _ => return None,
        }
    }
    (!parsed.head.is_empty()).then_some(parsed)
}

fn default_branch(directory: &Path) -> String {
    git::text(directory, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
        .and_then(|reference| reference.trim().strip_prefix("refs/remotes/origin/").map(str::to_owned))
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "main".to_owned())
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(arguments) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let repo = if arguments.repo.is_empty() { ".".to_owned() } else { arguments.repo };
    let directory = Path::new(&repo);
    if arguments.base.is_empty() || arguments.destination != default_branch(directory) {
        return 0;
    }
    let lane = if arguments.lane.is_empty() { arguments.head.clone() } else { arguments.lane };
    stale_revert::run(&["--repo".to_owned(), repo, "--lane".to_owned(), lane, arguments.base, arguments.head])
}
