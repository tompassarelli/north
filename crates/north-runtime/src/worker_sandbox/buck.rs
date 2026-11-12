use std::path::PathBuf;

use super::Context;
use super::removable::lanes;

const BUCK_OUT: &str = "buck-out";

/// The lanes a command run from `context` works in that are Buck2 projects, by the `.buckconfig` at their root.
fn buck_lanes(context: &Context) -> impl Iterator<Item = PathBuf> {
    lanes(context).into_iter().filter(|lane| lane.join(".buckconfig").is_file())
}

/// Makes `buck-out/` at the root of each Buck2 lane a command run from `context` works in, where missing. A Buck2 daemon outlives the command that starts it and keeps that command's domain, and a Landlock rule holds an inode, so a `buck-out/` the daemon's first build made would hold no rule of that domain and the daemon could not run what it builds there. Made first, it is one more entry at the lane root, executable, writable and removable as the rest of the lane is, beside the `target/` and `node_modules/` the domain refuses to run.
pub fn make_build_output(context: &Context) {
    for lane in buck_lanes(context) {
        let _ = std::fs::create_dir(lane.join(BUCK_OUT));
    }
}

/// The Buck2 daemon directory of each Buck2 lane a command run from `context` works in, made first where missing: `~/.buck/buckd/<real lane path>`, where Buck2 keeps that lane's daemon state for every isolation dir, placed from HOME alone with no setting to move it. A Landlock rule holds an inode, and the daemon reads its own `buckd.pid` through the domain of the command that started it, so the directory must exist before the domain is built. It is the one directory under `~/.buck` a command is given.
pub fn daemon_dirs(context: &Context) -> Vec<PathBuf> {
    if !context.home.is_absolute() {
        return Vec::new();
    }
    let buckd = context.home.join(".buck/buckd");
    buck_lanes(context)
        .filter_map(|lane| {
            let daemon = buckd.join(lane.canonicalize().ok()?.strip_prefix("/").ok()?);
            std::fs::create_dir_all(&daemon).ok().map(|()| daemon)
        })
        .collect()
}

#[cfg(test)]
#[path = "buck_tests.rs"]
mod tests;
