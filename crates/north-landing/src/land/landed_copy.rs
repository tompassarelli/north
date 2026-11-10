use std::collections::{HashMap, HashSet};

pub const FIELDS: &str = "--format=%H%x1f%at%x1f%ae%x1f%s";

#[derive(Debug, Eq, PartialEq)]
pub struct Landed {
    pub lane_commit: String,
    pub main_commit: String,
}

type Origin<'a> = (&'a str, &'a str, &'a str);

fn rows(listing: &str) -> impl Iterator<Item = (&str, Origin<'_>)> {
    listing.lines().filter_map(|line| {
        let mut fields = line.split('\x1f');
        Some((fields.next()?, (fields.next()?, fields.next()?, fields.next()?)))
    })
}

/// A lane commit that main already carries under another patch: the lane was amended after it landed, so picking it would apply the change a second time.
pub fn amended_after_landing(main: &str, lane: &str, unlanded: &HashSet<&str>) -> Option<Landed> {
    let landed: HashMap<Origin, &str> = rows(main).map(|(sha, origin)| (origin, sha)).collect();
    rows(lane)
        .filter(|(sha, _)| unlanded.contains(sha))
        .find_map(|(sha, origin)| landed.get(&origin).map(|main_commit| Landed { lane_commit: sha.to_owned(), main_commit: (*main_commit).to_owned() }))
}

pub fn refusal(landed: &Landed, base: &str) -> String {
    format!(
        "lane commit {} was already landed as {} on main@{base} and this head changes it again, so picking it would land the change twice; rebase onto main and queue only what is new on top of {}; NOT landed.",
        landed.lane_commit, landed.main_commit, landed.main_commit
    )
}
