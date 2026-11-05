use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use super::{Detector, Finding, Limits};

static FAILED_TEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^test (\S+) \.\.\. FAILED").expect("failed test pattern"));
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+").expect("url pattern"));
static VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b|\d+").expect("variable pattern"));
static BLOCKING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:failed )?\d+ blocking check\(s\) failed: (.+)$").expect("blocking pattern"));
const GENERIC_GATE: &str = "land checks";
const BUSY: [&str; 2] = ["text file busy", "etxtbsy"];

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

fn classes(text: &str) -> BTreeSet<String> {
    let lower = text.to_lowercase();
    let mut found = BTreeSet::new();
    if BUSY.iter().any(|needle| lower.contains(needle)) {
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
                done_when: format!("a lane lands on origin/main that removes the cause, and `north flow detect --dry-run` prints no `refusal:{class}` line for 24 h of new landings."),
                instances,
            }
        })
        .collect()
}
