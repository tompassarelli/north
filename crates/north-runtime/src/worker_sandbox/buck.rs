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

#[cfg(test)]
#[path = "buck_tests.rs"]
mod tests;
