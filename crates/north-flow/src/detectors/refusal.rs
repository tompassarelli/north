use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use super::{Detector, Finding, Limits};

static FAILED_TEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^test (\S+) \.\.\. FAILED").expect("failed test pattern"));
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+").expect("url pattern"));
static VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b|\d+").expect("variable pattern"));
static PASSING_TEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^test \S+ \.\.\. (?:ok|ignored)\b").expect("passing test pattern"));
static BLOCKING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:failed )?\d+ blocking check\(s\) failed: (.+)$").expect("blocking pattern"));
const GENERIC_GATE: &str = "land checks";
const BUSY: [&str; 2] = ["text file busy", "etxtbsy"];
const BUSY_OS_ERROR: &str = "(os error 26)";
const FAILURE_WORDS: [&str; 4] = ["error", "failed", "cannot", "could not"];

pub struct Refusal {
    pub lane: String,
    pub sha: String,
    pub at: f64,
    pub text: String,
}

#[derive(Default)]
pub struct Facts {
    pub owner: Option<String>,
    pub refusals: Vec<Refusal>,
}

fn normalized(detail: &str) -> String {
    let plain = URL.replace_all(detail, "");
    let plain = VARIABLE.replace_all(&plain, "#");
    plain.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect()
}

fn gates(text: &str) -> Vec<String> {
    let mut named: Vec<String> = Vec::new();
    for line in text.lines() {
        match BLOCKING.captures(line.trim()) {
            Some(found) => named.extend(found[1].split("; ").map(|gate| format!("gate:{}", gate.trim()))),
            None if line.contains("Autoland did not land") => named.push(format!("gate:{}", normalized(line))),
            None => {}
        }
    }
    named
}

fn reports_busy_executable(text: &str) -> bool {
    text.lines().map(|line| line.trim().to_lowercase()).filter(|line| !PASSING_TEST.is_match(line)).any(|line| {
        line.contains(BUSY_OS_ERROR) || (BUSY.iter().any(|needle| line.contains(needle)) && FAILURE_WORDS.iter().any(|word| line.contains(word)))
    })
}

fn classes(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if reports_busy_executable(text) {
        found.insert("text-file-busy".to_owned());
    }
    found.extend(FAILED_TEST.captures_iter(text).map(|test| format!("test:{}", &test[1])));
    let specific = !found.is_empty();
    found.extend(gates(text).into_iter().filter(|gate| !(specific && gate == &format!("gate:{GENERIC_GATE}"))));
    found
}

pub fn repeated(facts: &Facts, now: f64, limits: &Limits) -> Vec<Finding> {
    let mut merged: BTreeMap<(&str, &str), (f64, String)> = BTreeMap::new();
    for refusal in facts.refusals.iter().filter(|refusal| (now - limits.refusal_window_s..=now).contains(&refusal.at)) {
        let entry = merged.entry((refusal.lane.as_str(), refusal.sha.as_str())).or_insert((refusal.at, String::new()));
        entry.0 = entry.0.min(refusal.at);
        entry.1.push_str(&refusal.text);
        entry.1.push('\n');
    }
    let mut grouped: BTreeMap<String, Vec<(&str, &str, f64)>> = BTreeMap::new();
    for ((lane, sha), (at, text)) in &merged {
        for class in classes(text) {
            grouped.entry(class).or_default().push((*lane, *sha, *at));
        }
    }
    grouped
        .into_iter()
        .filter(|(_, refusals)| refusals.len() >= limits.refusal_min)
        .map(|(class, mut refusals)| {
            refusals.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.0.cmp(b.0)));
            let first = crate::foundation::clock::Timestamp::from_unix_seconds_f64(refusals[0].2).iso();
            let instances: Vec<String> = refusals.iter().map(|(lane, sha, at)| format!("{lane} {} ({})", sha.chars().take(9).collect::<String>(), crate::foundation::clock::Timestamp::from_unix_seconds_f64(*at).iso())).collect();
            Finding {
                detector: Detector::RepeatedRefusal,
                key: format!("refusal:{class}:{}", &first[..10]),
                owner: facts.owner.clone(),
                title: format!("Remove the recurring lander refusal {class}"),
                detail: format!("{} lander refusals within {:.0} h share the class {class}: {}.", refusals.len(), limits.refusal_window_s / 3600.0, instances.join("; ")),
                done_when: format!("a lane lands on origin/main that removes the cause, and `north ledger flow detect --dry-run` prints no `refusal:{class}` line for 24 h of new landings."),
                instances,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_whose_passing_output_mentions_etxtbsy_is_not_classed_text_file_busy_but_a_busy_executable_error_is() {
        let failed = "test shim::etxtbsy_retries_the_write ... ok\nretrying after ETXTBSY\ntest lander::lands ... FAILED\n";
        assert_eq!(classes(failed), BTreeSet::from(["test:lander::lands".to_owned()]));
        let busy = "test shim::etxtbsy_retries_the_write ... ok\nerror: failed to run script: Text file busy (os error 26)\n";
        assert_eq!(classes(busy), BTreeSet::from(["text-file-busy".to_owned()]));
    }
}
