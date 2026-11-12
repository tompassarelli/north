use std::path::PathBuf;

use north_base::foundation::offload::{ROOT_DEFAULT, SLOT_RECORDS};

use super::Context;
use super::removable::lanes;

const BUCK_OUT: &str = "buck-out";

/// The Buck2 projects a command run from `context` works in, by the `.buckconfig` at their root: its lanes, and the offload slot it runs in.
fn buck_lanes(context: &Context) -> impl Iterator<Item = PathBuf> {
    lanes(context).into_iter().chain(slot(context)).filter(|lane| lane.join(".buckconfig").is_file())
}

/// The offload slot a command runs in, when the offload leased that slot to the shell the command runs in (`NORTH_OFFLOAD_SLOT`) and its working directory lies in it: the variable and the directory together carry the lease. The offload's record of the slot, kept only while a job holds it, shuts out a stale or unknown name and proves no lease by itself. A land check's slot is never one: its buck client is the lander's, outside any domain. The offload removes a slot's daemon state whenever a job takes the slot (land_offload/ship/script.rs) and its lease ends every process of the job, so a daemon and what it left never pass from one job's domain to the next.
fn slot(context: &Context) -> Option<PathBuf> {
    let root = slots_root(context);
    let first = context.cwd.strip_prefix(&root).ok()?.components().next()?;
    let slot = root.join(first);
    let leased = context.offload_slot.as_ref()?;
    (*leased == slot && !context.land_check && context.home.join(SLOT_RECORDS).join(first).is_file()).then_some(slot)
}

fn slots_root(context: &Context) -> PathBuf {
    context.home.join(ROOT_DEFAULT)
}

/// Makes `buck-out/` at the root of each Buck2 lane a command run from `context` works in, where missing. A Buck2 daemon outlives the command that starts it and keeps that command's domain, and a Landlock rule holds an inode, so a `buck-out/` the daemon's first build made would hold no rule of that domain and the daemon could not run what it builds there. Made first, it is one more entry at the lane root, executable, writable and removable as the rest of the lane is, beside the `target/` and `node_modules/` the domain refuses to run.
pub fn make_build_output(context: &Context) {
    for lane in buck_lanes(context) {
        let _ = std::fs::create_dir(lane.join(BUCK_OUT));
    }
}

/// The Buck2 daemon directories a command run from `context` needs, made first where missing: `~/.buck/buckd/<real path>` of each Buck2 lane and of the offload slot it runs in, where Buck2 keeps that project's daemon state for every isolation dir, placed from HOME alone with no setting to move it. Every directory above them, the one that holds the slots' daemons included, is made here too, outside the domain, so a worker is granted only its own project's directory and writes nothing beside it. A Landlock rule holds an inode, and the daemon reads its own `buckd.pid` through the domain of the command that started it, so the directories must exist before the domain is built.
pub fn daemon_dirs(context: &Context) -> Vec<PathBuf> {
    if !context.home.is_absolute() {
        return Vec::new();
    }
    let buckd = context.home.join(".buck/buckd");
    buck_lanes(context)
        .filter_map(|project| {
            let daemon = buckd.join(project.canonicalize().ok()?.strip_prefix("/").ok()?);
            std::fs::create_dir_all(&daemon).ok().map(|()| daemon)
        })
        .collect()
}

#[cfg(test)]
#[path = "buck_tests.rs"]
mod tests;
