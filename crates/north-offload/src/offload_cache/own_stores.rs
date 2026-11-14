//! North's own stores under the state directory, pruned on the disk pass because a worker sandbox cannot write there: land-queue logs older than `LOG_RETENTION`, and the runtimes past the newest `RUNTIMES_KEPT` that no link, live process or lease needs, on a host that is a runtime source only, because a lander ships checks that run its own runtime on other hosts where no link, process or lease names it.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use north_base::platform::land_queue;
use north_runtime_follow::{collection, layout::Layout};

/// A guess; what a landing's follow-up reads from a log settles it.
const LOG_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// A guess; how far back a promotion rollback reaches settles it.
const RUNTIMES_KEPT: usize = 10;

pub fn prune(state_home: &Path, proc_root: &Path, now: SystemTime, runtimes: bool) {
    let logs = prune_logs(state_home, now);
    if logs > 0 {
        eprintln!("north capacity: removed {logs} land-queue logs older than {} days", LOG_RETENTION.as_secs() / 86_400);
    }
    if !runtimes {
        return;
    }
    let layout = Layout { state: state_home.join("north") };
    match collection::prune(&layout, proc_root, RUNTIMES_KEPT) {
        Ok(removed) => removed.iter().for_each(|name| eprintln!("north capacity: removed the runtime {name}")),
        Err(error) => eprintln!("north capacity: could not prune the runtimes: {error}"),
    }
}

fn prune_logs(state_home: &Path, now: SystemTime) -> usize {
    let queues = land_queue::queues(state_home);
    let Ok(root) = fs::canonicalize(&queues) else { return 0 };
    let mut removed = 0;
    for queue in fs::read_dir(&root).into_iter().flatten().flatten() {
        let logs = queue.path().join("logs");
        let Ok(resolved) = fs::canonicalize(&logs) else { continue };
        if resolved != root.join(queue.file_name()).join("logs") {
            continue;
        }
        for log in fs::read_dir(&resolved).into_iter().flatten().flatten() {
            let Ok(metadata) = fs::symlink_metadata(log.path()) else { continue };
            let old = metadata.modified().ok().and_then(|modified| now.duration_since(modified).ok()).is_some_and(|age| age > LOG_RETENTION);
            if metadata.is_file() && old && fs::remove_file(log.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}
