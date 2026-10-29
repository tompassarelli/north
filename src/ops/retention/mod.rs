mod archive;
mod family;
mod inventory;
mod plan;
mod remote;
mod store;
#[cfg(test)]
mod tests;

use std::env;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use family::Family;
use inventory::Entry;
use plan::Verdict;
use remote::{Remote, Ssh};

const USAGE: &str = "usage: north retain [--dry-run] [--host SSH_HOST] [--archive DIR]";
const DEFAULT_HOST: &str = "gex45";
const BATCH_BYTES: u64 = 256 << 20;
const BATCH_ENTRIES: usize = 200;

struct Policy {
    dry_run: bool,
    host: String,
    archive: PathBuf,
    batch_entries: usize,
    out_of_time: Box<dyn Fn() -> bool>,
}

#[derive(Default)]
struct Tally {
    entries: usize,
    bytes: u64,
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let started = Instant::now();
    let budget = family::minutes("retention_budget_minutes");
    let Some(policy) = policy(arguments, Box::new(move || started.elapsed() >= budget)) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let remote = Ssh { host: policy.host.clone() };
    let mut families = family::families();
    let mut failed = match store::families(&remote, &store::Layout::gex45(), family::hours("build_input_retention_hours")) {
        Ok(found) => {
            families.extend(found);
            false
        }
        Err(error) => {
            eprintln!("north retain: build-inputs: {error}");
            true
        }
    };
    let mut tally = Tally::default();
    for family in families {
        failed |= !retain(&remote, &family, &policy, now(), &mut tally);
    }
    let seconds = started.elapsed().as_secs_f64();
    println!("archived {} entries ({} MiB) in {seconds:.0} s, {:.2} MB/s", tally.entries, tally.bytes >> 20, tally.bytes as f64 / 1e6 / seconds.max(1.0));
    u8::from(failed)
}

fn policy(arguments: &[String], out_of_time: Box<dyn Fn() -> bool>) -> Option<Policy> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let mut policy = Policy { dry_run: false, host: DEFAULT_HOST.into(), archive: home.join("archive/smashcraft-gex45"), batch_entries: BATCH_ENTRIES, out_of_time };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--dry-run" => policy.dry_run = true,
            "--host" => policy.host = rest.next()?.clone(),
            "--archive" => policy.archive = PathBuf::from(rest.next()?),
            _ => return None,
        }
    }
    Some(policy)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn retain(remote: &dyn Remote, family: &Family, policy: &Policy, now: u64, tally: &mut Tally) -> bool {
    let (entries, held) = match (inventory::read(remote, family), inventory::held(remote, family)) {
        (Ok(entries), Ok(held)) => (entries, held),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("north retain: {}: {error}", family.name);
            return false;
        }
    };
    let destination = policy.archive.join(&family.name);
    if !policy.dry_run && let Err(error) = std::fs::create_dir_all(&destination) {
        eprintln!("north retain: {}: {error}", destination.display());
        return false;
    }
    let max_age = family.max_age.as_secs();
    let mut verdicts: Vec<_> = entries.iter().map(|entry| (entry, plan::judge(entry, &held, now, max_age))).collect();
    verdicts.sort_by_key(|(entry, _)| entry.newest);
    let mut expired = Vec::new();
    for (entry, verdict) in verdicts {
        match verdict {
            Verdict::Live | Verdict::Unverifiable => println!("kept {}/{}: {}", family.name, entry.name, verdict.reason()),
            Verdict::Archive if policy.dry_run => println!("would archive {}/{} ({} MiB): {}", family.name, entry.name, entry.bytes >> 20, verdict.reason()),
            Verdict::Archive => expired.push(entry),
            Verdict::Pinned | Verdict::Recent => {}
        }
    }
    let mut ok = true;
    let mut rest = expired.as_slice();
    while !rest.is_empty() {
        if (policy.out_of_time)() {
            println!("left {}: {} entries ({} MiB) for the next run; the time budget is spent", family.name, rest.len(), rest.iter().map(|entry| entry.bytes).sum::<u64>() >> 20);
            break;
        }
        let (batch, after) = rest.split_at(batch_len(rest, policy.batch_entries));
        rest = after;
        let names: Vec<&str> = batch.iter().map(|entry| entry.name.as_str()).collect();
        let outcomes = archive::archive(remote, family, &names, &destination);
        for entry in batch {
            match outcomes.get(&entry.name) {
                Some(Ok(())) => {
                    tally.entries += 1;
                    tally.bytes += entry.bytes;
                    println!("archived {}/{} ({} MiB) to {}, sha256 verified, source deleted", family.name, entry.name, entry.bytes >> 20, destination.display());
                }
                Some(Err(error)) => {
                    eprintln!("north retain: {}: {}: {error}", family.name, entry.name);
                    ok = false;
                }
                None => {
                    eprintln!("north retain: {}: {}: no outcome from its batch", family.name, entry.name);
                    ok = false;
                }
            }
        }
    }
    ok
}

fn batch_len(entries: &[&Entry], most: usize) -> usize {
    let mut bytes = 0;
    let fits = entries.iter().take(most).take_while(|entry| {
        bytes += entry.bytes;
        bytes <= BATCH_BYTES
    });
    fits.count().max(1)
}
