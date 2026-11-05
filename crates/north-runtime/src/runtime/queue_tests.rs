use super::{Entry, next};

fn entry(sha: &str, failed: bool) -> Entry {
    Entry { sha: sha.to_owned(), failed }
}

fn contains(older: &str, newer: &str) -> bool {
    ["c1", "c2", "c3"].iter().position(|sha| *sha == older) <= ["c1", "c2", "c3"].iter().position(|sha| *sha == newer)
}

#[test]
fn the_newest_skipped_predecessor_is_built_when_the_newer_build_fails() {
    let queue = [entry("c3", true), entry("c2", false), entry("c1", false)];
    assert_eq!(next(&queue, contains), Some("c2"));
}

