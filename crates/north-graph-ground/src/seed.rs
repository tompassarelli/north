use std::sync::LazyLock;

use regex::Regex;

const MAX_SEEDS: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Seed {
    Issue(String),
    File(String),
}

static ISSUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\w/#.-])(?:[\w-]+/)?([a-z][a-z0-9-]*#\d+)\b").expect("issue pattern"));
static FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\s`'\x22(\[])((?:[\w.-]+/)+[\w.-]*\w\.[A-Za-z][A-Za-z0-9]{0,4})\b").expect("file pattern"));

pub fn seeds(text: &str) -> Vec<Seed> {
    let mut found: Vec<Seed> = Vec::new();
    let issues = ISSUE.captures_iter(text).map(|captures| Seed::Issue(captures[1].to_owned()));
    let files = FILE.captures_iter(text).map(|captures| Seed::File(captures[1].trim_start_matches("./").to_owned()));
    for seed in issues.chain(files) {
        if !found.contains(&seed) {
            found.push(seed);
        }
    }
    found.truncate(MAX_SEEDS);
    found
}
