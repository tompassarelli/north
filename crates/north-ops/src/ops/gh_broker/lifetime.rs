//! When a broker stops serving: a resident one once its socket is taken or the live runtime moves, an on-demand one
//! once idle, wanted gone, superseded or replaced and no call is open.
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::server;
use crate::platform::gh_broker_route::Place;

const IDLE_EXIT: Duration = Duration::from_secs(30 * 60);

pub(super) fn watch(place: &Place, watched: Arc<server::Activity>, resident: bool) {
    let runtime = if resident { north_runtime_follow::Follow::current() } else { north_runtime_follow::Follow::service() };
    watch_runtime(place, watched, resident, runtime);
}

pub(super) fn watch_runtime(place: &Place, watched: Arc<server::Activity>, resident: bool, mut runtime: north_runtime_follow::Follow) {
    let (socket, wanted) = (place.socket(), place.resident_wanted());
    let bound = inode(&socket);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let superseded = inode(&socket) != bound;
            let moved = runtime.replaced();
            if resident {
                if superseded {
                    eprintln!("north gh-broker: {} is no longer this broker's socket; exiting so the service binds it again", socket.display());
                    std::process::exit(1);
                }
                if let Some(next) = moved {
                    eprintln!("north gh-broker: the live runtime changed to {next}; finishing open calls so the service restarts on it");
                    watched.drain(&socket);
                    return;
                }
                continue;
            }
            let Some(idle) = watched.idle_for() else {
                continue;
            };
            if superseded || wanted.exists() || idle >= IDLE_EXIT || moved.is_some() {
                if !superseded {
                    let _ = std::fs::remove_file(&socket);
                }
                std::process::exit(0);
            }
        }
    });
}

fn inode(path: &Path) -> Option<(u64, u64)> {
    std::fs::symlink_metadata(path).ok().map(|meta| (meta.dev(), meta.ino()))
}
