use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, PartialEq, Eq)]
pub enum Blame {
    Lane { lane: String, sha: String },
    Unjudged(String),
}

pub fn responsible(chain: &[String], runs: &BTreeMap<String, BTreeSet<String>>, landed: &BTreeMap<String, Vec<String>>, test: &str) -> Blame {
    let failed = |sha: &String| runs.get(sha).is_some_and(|tests| tests.contains(test));
    let Some(first) = chain.iter().position(failed) else {
        return Blame::Unjudged(format!("no full run in the range failed {test}"));
    };
    let start = chain[..first].iter().rposition(|sha| runs.contains_key(sha)).map_or(0, |passed| passed + 1);
    let suspects = &chain[start..=first];
    let mut lanes: Vec<(&str, &str)> = Vec::new();
    for sha in suspects {
        let Some(names) = landed.get(sha) else {
            return Blame::Unjudged(format!("{} has no landing row naming its lanes", &sha[..sha.len().min(12)]));
        };
        lanes.extend(names.iter().map(|lane| (lane.as_str(), sha.as_str())));
    }
    match lanes.as_slice() {
        [(lane, sha)] => Blame::Lane { lane: (*lane).to_owned(), sha: (*sha).to_owned() },
        _ => Blame::Unjudged(format!("{} lanes landed between the last full run that passed {test} and the first that failed it ({}), so only the lander's bisection can name one", lanes.len(), lanes.iter().map(|(lane, _)| *lane).collect::<Vec<_>>().join(", "))),
    }
}
