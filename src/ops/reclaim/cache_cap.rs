use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use super::held::Held;
use super::{Inputs, Policy, Removal, hours, removed};
use crate::platform::shared_target::tree;

const GIB: u64 = 1 << 30;
const CACHE_IDLE: Duration = Duration::from_secs(4 * 3600);
const CAPPED: [(&str, u64); 2] = [("nexus-run", 2 * GIB), ("galileo-full-suite/target", 4 * GIB)];

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let mut removals = Vec::new();
    for (relative, cap) in CAPPED {
        removals.extend(trim(inputs, policy, held, &inputs.cache.join(relative), cap));
    }
    Ok(removals)
}

fn trim(inputs: &Inputs, policy: &Policy, held: &Held, root: &Path, cap: u64) -> Vec<Removal> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut children = Vec::new();
    for entry in entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())) {
        if let Ok((bytes, touched)) = tree(&entry.path(), &mut HashSet::new()) {
            children.push((touched, bytes, entry.path()));
        }
    }
    children.sort();
    let mut size: u64 = children.iter().map(|(_, bytes, _)| bytes).sum();
    let limit = policy.idle.min(CACHE_IDLE);
    let mut removals = Vec::new();
    for (touched, bytes, path) in children {
        if size <= cap {
            break;
        }
        let idle = inputs.now.duration_since(touched).unwrap_or_default();
        if idle < limit || held.within(&path) || (!policy.dry_run && !removed(&path)) {
            continue;
        }
        size = size.saturating_sub(bytes);
        let reason = format!("{} over its {} GiB cap, oldest first: idle {}h, held by no process; the next job restores it", root.display(), cap / GIB, hours(idle));
        removals.push(Removal { path, reason, bytes });
    }
    removals
}
