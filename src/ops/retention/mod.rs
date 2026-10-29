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
use std::time::{SystemTime, UNIX_EPOCH};

use family::Family;
use plan::Verdict;
use remote::{Remote, Ssh};

const USAGE: &str = "usage: north retain [--dry-run] [--host SSH_HOST] [--archive DIR]";
const DEFAULT_HOST: &str = "gex45";

struct Policy {
    dry_run: bool,
    host: String,
    archive: PathBuf,
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let Some(policy) = policy(arguments) else {
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
    for family in families {
        failed |= !retain(&remote, &family, &policy, now());
    }
    u8::from(failed)
}

fn policy(arguments: &[String]) -> Option<Policy> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let mut policy = Policy { dry_run: false, host: DEFAULT_HOST.into(), archive: home.join("archive/smashcraft-gex45") };
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

fn retain(remote: &dyn Remote, family: &Family, policy: &Policy, now: u64) -> bool {
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
    let mut ok = true;
    for (entry, verdict) in verdicts {
        let mib = entry.bytes / (1 << 20);
        match verdict {
            Verdict::Live | Verdict::Unverifiable => println!("kept {}/{}: {}", family.name, entry.name, verdict.reason()),
            Verdict::Archive if policy.dry_run => println!("would archive {}/{} ({mib} MiB): {}", family.name, entry.name, verdict.reason()),
            Verdict::Archive => match archive::archive(remote, family, &entry.name, &destination) {
                Ok(()) => println!("archived {}/{} ({mib} MiB) to {}, sha256 verified, source deleted", family.name, entry.name, destination.display()),
                Err(error) => {
                    eprintln!("north retain: {}: {error}", family.name);
                    ok = false;
                }
            },
            Verdict::Pinned | Verdict::Recent => {}
        }
    }
    ok
}
