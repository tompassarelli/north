use std::collections::BTreeMap;

use crate::threads::incidents::corrections::{Class, UNCLASSIFIED};

use super::{Detector, Finding};
use crate::foundation::clock::Timestamp;

const SEEN_MIN: usize = 2;

#[derive(Default)]
pub struct Facts {
    pub seen: Vec<(String, f64)>,
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
                done_when: format!("the guard lands on origin/main and harness/corrections.toml sets guarded_at for {name} to its landing time, so a later {name} correction prints red in the proxy's fleet tick."),
                instances,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::incidents::corrections::parse;

    const TABLE: &str = "[[class]]\nname = \"pace\"\npattern = 'too slow'\nparaphrase = \"Tom said the work was moving too slowly.\"\nowner = \"north#636\"\nguard = \"A Stop hook names idle capacity.\"\n";

    #[test]
    fn a_class_seen_twice_proposes_its_guard_on_its_owner_and_once_or_unclassified_does_not() {
        let classes = parse(TABLE).unwrap();
        let facts = Facts { seen: vec![("pace".into(), 1_762_400_000.0), ("pace".into(), 1_762_300_000.0), ("overbuilt".into(), 1_762_300_000.0), (UNCLASSIFIED.into(), 1.0), (UNCLASSIFIED.into(), 2.0)] };
        let found = repeated(&facts, &classes);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "correction:pace");
        assert_eq!(found[0].owner.as_deref(), Some("north#636"));
        assert!(found[0].box_line().contains("Proposed guard: A Stop hook names idle capacity."));
        assert!(found[0].detail.starts_with("Tom corrected this class 2 times (2025-11-04T23:46:40Z, 2025-11-06T03:33:20Z)"));
    }
}
