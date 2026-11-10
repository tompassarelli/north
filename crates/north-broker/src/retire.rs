//! The user broker's lifetime: it serves on demand, so it exits once idle or once its runtime is replaced, and the next
//! client starts it on the runtime its link names.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::server::Broker;

const POLL: Duration = Duration::from_secs(5);

/// Exits once no request is in flight and the broker has been idle past `limit` or its runtime was replaced.
pub fn when_idle(broker: &Arc<Broker>, socket: PathBuf, limit: Duration) {
    let watched = Arc::clone(broker);
    let mut runtime = north_runtime_follow::Follow::current();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(POLL);
            let replaced = runtime.replaced().is_some();
            if watched.idle().is_some_and(|idle| replaced || idle > limit) {
                let _ = std::fs::remove_file(&socket);
                std::process::exit(0);
            }
        }
    });
}
