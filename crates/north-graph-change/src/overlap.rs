use std::collections::BTreeSet;

use crate::cone::Cone;
use crate::facts::TestId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub tests: BTreeSet<TestId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlap {
    pub groups: Vec<Vec<String>>,
    pub pairs: Vec<Pair>,
    pub always: BTreeSet<TestId>,
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

pub fn of(lanes: &[(String, Cone)], all_tests: impl FnOnce() -> BTreeSet<TestId>) -> Overlap {
    let mut parents: Vec<usize> = (0..lanes.len()).collect();
    let mut pairs = Vec::new();
    for (left, (a, cone_a)) in lanes.iter().enumerate() {
        for (right, (b, cone_b)) in lanes.iter().enumerate().skip(left + 1) {
            let joined = cone_a.untraced.is_some() || cone_b.untraced.is_some();
            let tests: BTreeSet<TestId> = cone_a.tests.intersection(&cone_b.tests).cloned().collect();
            if !joined && tests.is_empty() {
                continue;
            }
            let (left_root, right_root) = (root(&mut parents, left), root(&mut parents, right));
            parents[right_root] = left_root;
            if !tests.is_empty() {
                pairs.push(Pair { a: a.clone(), b: b.clone(), tests });
            }
        }
    }
    let mut groups: Vec<(usize, Vec<String>)> = Vec::new();
    for (index, (name, _)) in lanes.iter().enumerate() {
        let group = root(&mut parents, index);
        match groups.iter_mut().find(|(known, _)| *known == group) {
            Some((_, names)) => names.push(name.clone()),
            None => groups.push((group, vec![name.clone()])),
        }
    }
    let mut always: BTreeSet<TestId> = lanes.iter().flat_map(|(_, cone)| cone.always.iter().cloned()).collect();
    if lanes.iter().any(|(_, cone)| cone.untraced.is_some()) {
        always.extend(all_tests());
    }
    Overlap { groups: groups.into_iter().map(|(_, names)| names).collect(), pairs, always }
}
