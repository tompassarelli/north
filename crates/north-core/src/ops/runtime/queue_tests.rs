use super::{Entry, next};

fn entry(sha: &str, failed: bool) -> Entry {
    Entry { sha: sha.to_owned(), failed }
}

fn contains(older: &str, newer: &str) -> bool {
    ["c1", "c2", "c3"].iter().position(|sha| *sha == older) <= ["c1", "c2", "c3"].iter().position(|sha| *sha == newer)
}

#[test]
fn a_build_is_skipped_while_a_newer_build_that_contains_it_is_queued() {
    let queue = [entry("c3", false), entry("c2", false), entry("c1", false)];
    assert_eq!(next(&queue, contains), Some("c3"));
}

#[test]
fn the_newest_skipped_predecessor_is_built_when_the_newer_build_fails() {
    let queue = [entry("c3", true), entry("c2", false), entry("c1", false)];
    assert_eq!(next(&queue, contains), Some("c2"));
}

#[test]
fn a_build_is_not_skipped_for_a_newer_build_that_has_already_failed() {
    let queue = [entry("c2", false), entry("c3", true)];
    assert_eq!(next(&queue, contains), Some("c2"));
}

#[test]
fn nothing_is_built_when_every_queued_build_has_failed() {
    let queue = [entry("c2", true), entry("c1", true)];
    assert_eq!(next(&queue, contains), None);
}
