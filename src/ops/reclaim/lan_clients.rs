use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;

use rustix::fs::{FlockOperation, flock};

use super::held::Held;
use super::{Inputs, KEPT_PAIRS, Policy, Removal, hours, idle_for, removed};

pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let mut removals = Vec::new();
    for (pair, clients) in pairs(inputs)? {
        let state = inputs.lan_state.join(format!("pair-{pair}"));
        if KEPT_PAIRS.contains(&pair) || queued(&state) || clients.iter().any(|client| held.within(client)) {
            continue;
        }
        let activity = if state.is_dir() { state.clone() } else { clients[0].clone() };
        let Ok(idle) = idle_for(&activity, inputs.now) else { continue };
        if idle < policy.idle {
            continue;
        }
        fs::create_dir_all(&state)?;
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(state.join("pool.lock"))?;
        if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
            continue;
        }
        for client in clients {
            if policy.dry_run || removed(&client) {
                removals.push(Removal { path: client, reason: format!("Wisp LAN pair {pair} idle {}h and unclaimed; `wisp lan setup --pair {pair}` rebuilds it", hours(idle)) });
            }
        }
    }
    Ok(removals)
}

fn pairs(inputs: &Inputs) -> io::Result<BTreeMap<u32, Vec<PathBuf>>> {
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

fn queued(state: &std::path::Path) -> bool {
    fs::read_dir(state.join("queue")).is_ok_and(|mut waiting| waiting.next().is_some())
}
