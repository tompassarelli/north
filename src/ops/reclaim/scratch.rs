use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;

use super::held::Held;
use super::{Inputs, Policy, Removal, hours, idle_for, removed};

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let mut removals = Vec::new();
    for entry in fs::read_dir(&inputs.tmp)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else { continue };
        if metadata.uid() != inputs.uid || !(metadata.is_dir() || metadata.is_file()) || display_server(&name) || held.within(&path) {
            continue;
        }
        let Ok(idle) = idle_for(&path, inputs.now) else { continue };
        if idle < policy.idle {
            continue;
        }
        if !policy.dry_run && !removed(&path) {
            continue;
        }
        removals.push(Removal { path, reason: format!("scratch idle {}h, held by no process", hours(idle)) });
    }
    Ok(removals)
}

fn display_server(name: &str) -> bool {
    name.starts_with(".X") || name.ends_with("-unix")
}
