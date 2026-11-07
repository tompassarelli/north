use super::{Detector, Finding};

const NEAR_CAP_PERCENT: usize = 95;
const SPLIT_TARGET_PERCENT: usize = 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Crate {
    pub name: String,
    pub lines: usize,
    pub cap: usize,
}

#[derive(Default)]
pub struct Facts {
    pub owner: Option<String>,
    pub crates: Vec<Crate>,
}

pub fn census(report: &str) -> Vec<Crate> {
    report
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["crate-lines:", name, lines, "non-test", "lines,", "cap", cap, ..] => Some(Crate { name: (*name).to_owned(), lines: lines.parse().ok()?, cap: cap.parse().ok()? }),
                _ => None,
            }
        })
        .collect()
}

pub fn near(facts: &Facts) -> Vec<Finding> {
    facts
        .crates
        .iter()
        .filter(|krate| krate.lines * 100 >= krate.cap * NEAR_CAP_PERCENT)
        .map(|krate| {
            let target = krate.cap * SPLIT_TARGET_PERCENT / 100;
            let standing = if krate.lines > krate.cap { format!("is over its {}-line cap at {} non-test lines, so every landing that grows it is refused", krate.cap, krate.lines) } else { format!("has {} non-test lines against its {}-line cap, so the next lanes would cross it", krate.lines, krate.cap) };
            Finding {
                detector: Detector::CrateHeadroom,
                key: format!("crate-headroom:{}:{}", krate.name, krate.cap),
                owner: facts.owner.clone(),
                title: format!("Split {} to about {SPLIT_TARGET_PERCENT}% of its line cap", krate.name),
                detail: format!("{} {standing}. Split it by concern into a crate it depends on (docs/rationale/crate-split.md) down to about {target} lines, {SPLIT_TARGET_PERCENT}% of the cap, so it is not back at the line a few lanes later.", krate.name),
                done_when: format!("`north code-health crate-lines` prints {} at or under {target} non-test lines.", krate.name),
                instances: vec![krate.name.clone()],
            }
        })
        .collect()
}
