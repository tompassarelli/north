use std::path::Path;

use super::super::enqueue::{Facts, default_branch};
use super::{Request, Road, checkout, normal};
use crate::land::scans::outgoing::denylist::Denylist;

/// The facts of the lane the call names, started before its paths are named and its files written, so the destination's fetch runs beside that work; none for a lane the road refuses, whose git locks a fetch could strand.
pub(super) fn early_facts(directory: &Path, request: &Request, road: &impl Road) -> Option<Facts> {
    let home = if request.files.is_empty() { normal(&directory.join(request.paths.first()?)) } else { directory.to_owned() };
    let root = checkout(&home)?;
    road.writable(&root).ok()?;
    Facts::start(&root, &default_branch(&root), &Denylist::from_env())
}
