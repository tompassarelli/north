use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

use super::held::Held;
use super::{Inputs, KEPT_PAIRS, Policy, Removal, hours, idle_for, removed, size};

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let mut removals = Vec::new();
    let pooled = pooled_pairs(&inputs.lan_state);
    for (pair, clients) in pairs(inputs)? {
        let state = inputs.lan_state.join(format!("pair-{pair}"));
        if KEPT_PAIRS.contains(&pair) || pooled.contains(&pair) || queued(&state) || clients.iter().any(|client| held.within(client)) {
            continue;
        }
        let activity = if state.is_dir() { state.clone() } else { clients[0].clone() };
        let Ok(idle) = idle_for(&activity, inputs.now) else { continue };
        if idle < policy.idle {
            continue;
        }
        let Some(_lock) = unclaimed(&state)? else { continue };
        for client in clients {
            let bytes = size(&client);
            if policy.dry_run || removed(&client) {
                removals.push(Removal { path: client, reason: format!("Wisp LAN pair {pair} idle {}h and unclaimed; `wisp lan setup --pair {pair}` rebuilds it", hours(idle)), bytes });
            }
        }
    }
    Ok(removals)
}

pub(super) fn unclaimed(state: &Path) -> io::Result<Option<File>> {
    fs::create_dir_all(state)?;
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(state.join("pool.lock"))?;
    Ok(flock(&lock, FlockOperation::NonBlockingLockExclusive).ok().map(|()| lock))
}

fn pooled_pairs(lan_state: &Path) -> BTreeSet<u32> {
    let listed = fs::read(lan_state.join("pool.json")).ok().and_then(|text| serde_json::from_slice::<serde_json::Value>(&text).ok());
    let ids = listed.as_ref().and_then(|pool| pool["pairs"].as_array()).into_iter().flatten().filter_map(|pair| pair["id"].as_u64());
    ids.filter_map(|id| u32::try_from(id).ok()).collect()
}

pub(super) fn pairs(inputs: &Inputs) -> io::Result<BTreeMap<u32, Vec<PathBuf>>> {
    let mut pairs = BTreeMap::<u32, Vec<PathBuf>>::new();
    let entries = match fs::read_dir(&inputs.lan_clients) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(pairs),
        Err(error) => return Err(error),
    };
    for entry in entries.flatten() {
        if let Some(pair) = pair_of(&entry.file_name().to_string_lossy()) {
            pairs.entry(pair).or_default().push(entry.path());
        }
    }
    Ok(pairs)
}

fn pair_of(client: &str) -> Option<u32> {
    let number = client.strip_prefix("lan")?.strip_suffix(['a', 'b'])?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

pub(super) fn queued(state: &Path) -> bool {
    fs::read_dir(state.join("queue")).is_ok_and(|mut waiting| waiting.any(|entry| entry.is_ok_and(|ticket| ticket.path().extension().is_some_and(|extension| extension == "json"))))
}
