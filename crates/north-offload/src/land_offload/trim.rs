//! `north capacity offload trim-target TARGET KEEP_MIB`: the slot script's step for a slot target past its cap, which keeps the build the
//! next land check reuses instead of removing the whole target (north#935).
use std::path::Path;
use std::time::SystemTime;

/// The verb's exit, its usage when its words do not parse.
pub(super) fn run(words: &[&str]) -> u8 {
    let Some((target, keep_mib)) = parse(words) else { return super::usage() };
    match north_base::platform::shared_target::trim_target(target, keep_mib << 20, SystemTime::now()) {
        Ok((before, freed)) => {
            eprintln!("offload: trimmed {} MiB of least recently used build output from {} ({} MiB to {} MiB)", freed >> 20, target.display(), before >> 20, before.saturating_sub(freed) >> 20);
            0
        }
        Err(error) => {
            eprintln!("offload: could not trim {}: {error}", target.display());
            1
        }
    }
}

fn parse<'a>(words: &[&'a str]) -> Option<(&'a Path, u64)> {
    let [target, keep_mib] = words else { return None };
    Some((Path::new(*target), keep_mib.parse().ok()?))
}
