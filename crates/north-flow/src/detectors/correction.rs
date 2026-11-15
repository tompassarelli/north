use std::collections::BTreeMap;

use crate::threads::incidents::corrections::{Class, UNCLASSIFIED};

use super::{Detector, Finding};
use crate::foundation::clock::Timestamp;

const SEEN_MIN: usize = 2;
#[derive(Default)]
pub struct Facts {
    pub seen: Vec<(String, f64)>,
}

pub fn seen(graph: &std::path::Path) -> Facts {
    if !graph.exists() {
        return Facts::default();
    }
    Facts {
        seen: north_graph_loops::corrections::seen(graph).unwrap_or_else(|error| {
            eprintln!("detectors: corrections unread from the graph: {error}");
            Vec::new()
        }),
    }
}

pub fn repeated(facts: &Facts, classes: &[Class]) -> Vec<Finding> {
    let mut grouped: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for (class, at) in facts.seen.iter().filter(|(class, _)| class != UNCLASSIFIED) {
        grouped.entry(class.as_str()).or_default().push(*at);
    }
    grouped
        .into_iter()
        .filter(|(_, seen)| seen.len() >= SEEN_MIN)
        .map(|(name, mut seen)| {
            seen.sort_by(f64::total_cmp);
            let class = classes.iter().find(|class| class.name == name);
            let guard = class.and_then(|class| class.guard.as_deref()).unwrap_or("none is named in harness/corrections.toml yet; name one there");
            let instances: Vec<String> = seen.iter().map(|at| Timestamp::from_unix_seconds_f64(*at).iso()).collect();
            Finding {
                detector: Detector::RepeatedCorrection,
                key: format!("correction:{name}"),
                owner: class.and_then(|class| class.owner.clone()),
                title: format!("Guard the correction class {name}"),
                detail: format!("Tom corrected this class {} times ({}), each fixed by hand. Proposed guard: {guard}", seen.len(), instances.join(", ")),
                done_when: format!("the guard lands on origin/main and harness/corrections.toml sets guarded_at for {name} to its landing time, so a later {name} correction prints red in the coordinator's fleet tick."),
                instances,
            }
        })
        .collect()
}
