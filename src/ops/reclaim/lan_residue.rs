use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::held::Held;
use super::lan_clients::{pairs, queued, unclaimed};
use super::{Inputs, Policy, Removal, hours, removed, size};

const DOCUMENTS: &str = "pfx/drive_c/users/steamuser/Documents/Warcraft III";
const RESIDUE: [&str; 2] = ["CustomMapData", "Maps/Wisp"];

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let mut removals = Vec::new();
    for (pair, clients) in pairs(inputs)? {
        let state = inputs.lan_state.join(format!("pair-{pair}"));
        if queued(&state) || clients.iter().any(|client| held.within(client)) {
            continue;
        }
        let Some(_lock) = unclaimed(&state)? else { continue };
        for client in clients {
            for relative in RESIDUE {
                let root = client.join(DOCUMENTS).join(relative);
                let stale: Vec<(PathBuf, Duration)> = files(&root).into_iter().filter_map(|file| Some((file.clone(), idle(&file, inputs)?))).filter(|(_, idle)| *idle >= policy.idle).collect();
                let Some(oldest) = stale.iter().map(|(_, idle)| *idle).min() else { continue };
                let bytes = stale.iter().map(|(file, _)| size(file)).sum();
                let count = stale.iter().filter(|(file, _)| policy.dry_run || removed(file)).count();
                if count > 0 {
                    removals.push(Removal { path: root, reason: format!("{count} map copies or custom map data files of Wisp LAN pair {pair}, none touched for {}h; the next match copies and writes them again", hours(oldest)), bytes });
                }
            }
        }
    }
    Ok(removals)
}

fn idle(file: &Path, inputs: &Inputs) -> Option<Duration> {
    let modified = fs::symlink_metadata(file).ok()?.modified().ok()?;
    Some(inputs.now.duration_since(modified).unwrap_or_default())
}

fn files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => found.extend(files(&entry.path())),
            Ok(kind) if kind.is_file() => found.push(entry.path()),
            _ => {}
        }
    }
    found
}
