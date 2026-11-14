use super::{Build, Profile};

/// Whether `request` bypasses the compile cache on the Buck2 side: when it says so, and always for the release build of the runtime programs, the binary every host installs, which Buck2 builds on the workers from source and reads no cached action result for. The Cargo side's `uncached` also drops the compile-cache wrapper, which a host without the key still builds with, so that request keeps it false.
pub(super) fn uncached(request: &Build) -> bool {
    request.uncached || (request.profile == Profile::Release && request.binary.is_none() && !request.programs.is_empty())
}
