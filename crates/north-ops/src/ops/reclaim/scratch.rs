use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::fleet::records::{RecordedSession, recorded_sessions};

use super::held::Held;
use super::{Inputs, Left, Policy, Reclaimed, Removal, hours, idle_for, removed, size};

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Reclaimed> {
    let named = named_scratch(inputs);
    let root = fs::canonicalize(&inputs.tmp)?;
    let mut done = Reclaimed::default();
    for entry in fs::read_dir(&inputs.tmp)?.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else { continue };
        if metadata.uid() != inputs.uid {
            continue;
        }
        let Some(sessions) = named.get(&path) else {
            done.left.push(Left { path, why: "no record names it".into() });
            continue;
        };
        if !metadata.is_dir() || fs::canonicalize(&path).ok().as_deref() != Some(&root.join(entry.file_name())) {
            done.left.push(Left { path, why: "it resolves outside the temporary root".into() });
            continue;
        }
        if let Some(live) = sessions.iter().find(|session| !session.ended) {
            done.left.push(Left { path, why: format!("session {} is not ended", live.name) });
            continue;
        }
        if held.within(&path) {
            done.left.push(Left { path, why: "a process holds it".into() });
            continue;
        }
        let Ok(idle) = idle_for(&path, inputs.now) else { continue };
        if idle < policy.idle {
            done.left.push(Left { path, why: format!("idle {}h, under the limit", hours(idle)) });
            continue;
        }
        let bytes = size(&path);
        if !policy.dry_run && !removed(&path) {
            continue;
        }
        let ended = &sessions[0];
        done.removals.push(Removal { path, reason: format!("scratch of ended session {} (record {})", ended.name, ended.file.display()), bytes });
    }
    Ok(done)
}

fn named_scratch(inputs: &Inputs) -> BTreeMap<PathBuf, Vec<RecordedSession>> {
    let mut named: BTreeMap<PathBuf, Vec<RecordedSession>> = BTreeMap::new();
    for session in recorded_sessions(&fleet_root(&inputs.state)) {
        named.entry(inputs.tmp.join(format!("north-{}-{}", session.provider, session.name))).or_default().push(session);
    }
    named
}

fn fleet_root(state: &Path) -> PathBuf {
    state.join("north/fleet")
}
