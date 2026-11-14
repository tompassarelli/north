use std::path::Path;

use crate::{Range, git};

pub fn ranges(directory: &Path, range: &str, tips: &str, whole: bool) -> Option<Vec<String>> {
    let Range { base, head } = Range::parse(range)?;
    if whole {
        return Some(vec![range.to_owned()]);
    }
    let commit = |revision: &str| git::text(directory, &["rev-parse", "--verify", &format!("{revision}^{{commit}}")]).map(|sha| sha.trim().to_owned());
    let (base, head) = (commit(&git::merge_base(directory, base, head))?, commit(head)?);
    let tips: Vec<String> = tips.split_whitespace().map(str::to_owned).collect();
    let ends = if tips.last().and_then(|last| commit(last)).is_some_and(|last| last == head) {
        tips
    } else {
        git::text(directory, &["rev-list", "--reverse", "--first-parent", &format!("{base}..{head}")])?.lines().map(str::to_owned).collect()
    };
    let mut from = base;
    let mut ranges = Vec::new();
    for end in ends {
        ranges.push(format!("{from}..{end}"));
        from = end;
    }
    Some(if ranges.is_empty() { vec![range.to_owned()] } else { ranges })
}
