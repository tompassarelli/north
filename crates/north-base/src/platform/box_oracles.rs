use regex::Regex;
use std::sync::LazyLock;

use super::cleanup;

static TICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t>]*[-*][ \t]+\[[xX]\][ \t]+(.*)$").expect("ticked box pattern"));
static PASSED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Evidence: `[^`]+` (?:exited 0|passed) on ").expect("oracle evidence pattern"));
static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bsplit\b[^.;]*?(?:[A-Za-z0-9_.-]+/)?[A-Za-z0-9_.-]+#\d+").expect("split owner pattern"));
static LANDED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*landed on origin/main\b").expect("landed box pattern"));

pub fn unproven(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| !cleanup::is_box(line))
        .filter_map(|line| TICKED.captures(line)?.get(1).map(|text| text.as_str().trim()))
        .filter(|text| !LANDED.is_match(text) && !PASSED.is_match(text) && !SPLIT.is_match(text))
        .collect()
}
