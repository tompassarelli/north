use super::super::layout::{self, Layout};
use super::{ancestor, fetch, usable};
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Shipped {
    Current,
    Older(String),
    Missing,
    Promote,
}

/// The primary checkout only answers whether the shipped runtime is older than the live one; a checkout that cannot fetch, is dirty or lacks the shipped commit leaves that unknown and the shipped runtime is promoted, since the host that shipped it already ordered it.
pub(super) fn shipped(layout: &Layout, checkout: &Path, target: &str, previous: Option<&Path>) -> Shipped {
    if previous == Some(layout::link(target).as_path()) {
        return Shipped::Current;
    }
    if let Err(failure) = fetch(checkout) {
        println!("north runtime: {} cannot fetch ({}); judging the shipped {} without it", checkout.display(), failure.message, layout::short(target));
    }
    match previous.and_then(layout::linked_sha).filter(|current| ancestor(checkout, target, current)) {
        Some(newer) => Shipped::Older(newer.to_owned()),
        None if !usable(&layout.runtime(target)) => Shipped::Missing,
        None => Shipped::Promote,
    }
}
