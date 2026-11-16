use std::collections::{BTreeMap, BTreeSet};

use super::Probed;
use crate::land::checks::Verdict;

type Search<'a> = (&'a str, usize, usize);

fn introduced(probed: &Probed, signature: &str) -> Option<bool> {
    match &probed.verdict {
        Verdict::RedMain(_) => Some(probed.failing.contains(signature)),
        Verdict::Green => Some(false),
        _ => None,
    }
}

/// Every lane whose prefix first reaches one of the red run's failure signatures: the signatures are searched level by level, one binary search each, and every prefix a search needs is checked once, so two lanes that break different checks are isolated in at most k·log2(n) runs for k such lanes. `round` checks prefixes by their count of landed lanes. None when a prefix reached no verdict or a signature first appears outside the batch, so the caller falls back to the single-culprit search.
pub(super) fn culprits(lanes: usize, signatures: &BTreeSet<String>, mut round: impl FnMut(&[usize]) -> Vec<Probed>) -> Option<Vec<usize>> {
    if lanes < 2 || signatures.is_empty() {
        return None;
    }
    let mut searches: Vec<Search> = signatures.iter().map(|signature| (signature.as_str(), 0, lanes)).collect();
    let mut checked: BTreeMap<usize, Probed> = BTreeMap::from([(lanes, Probed { verdict: Verdict::RedMain(String::new()), failing: signatures.clone() })]);
    loop {
        let wanted = wanted(&searches, &checked);
        if !wanted.is_empty() {
            let probes = round(&wanted);
            if probes.len() != wanted.len() {
                return None;
            }
            checked.extend(wanted.into_iter().zip(probes));
        }
        if !narrowed(&mut searches, &checked)? {
            return found(&searches, lanes);
        }
    }
}

fn wanted(searches: &[Search], checked: &BTreeMap<usize, Probed>) -> Vec<usize> {
    let mut wanted: Vec<usize> = searches.iter().filter(|(_, green, red)| green < red).map(|(_, green, red)| green + (red - green) / 2 + 1).filter(|count| !checked.contains_key(count)).collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
}

/// Halves each open search on the checked prefixes; false once every search has closed.
fn narrowed(searches: &mut [Search], checked: &BTreeMap<usize, Probed>) -> Option<bool> {
    let mut active = false;
    for (signature, green, red) in searches.iter_mut().filter(|(_, green, red)| green < red) {
        let middle = *green + (*red - *green) / 2;
        if introduced(checked.get(&(middle + 1))?, signature)? {
            *red = middle;
        } else {
            *green = middle + 1;
        }
        active = true;
    }
    Some(active)
}

fn found(searches: &[Search], lanes: usize) -> Option<Vec<usize>> {
    let mut found = Vec::new();
    for &(signature, green, _) in searches {
        if green >= lanes {
            println!("north land: the failing check {signature} first appears outside the landed lanes, so no lane is named for it");
            return None;
        }
        if !found.contains(&green) {
            found.push(green);
        }
    }
    Some(found)
}
