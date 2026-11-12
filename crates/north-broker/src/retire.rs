//! The user broker's lifetime: it serves on demand, so it exits once idle or once its runtime is replaced, and the next
//! client starts it on the runtime its link names. It serves every client of its user and outlives the one that
//! started it, so a link that names one runtime for good is not one it follows.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::server::Broker;

const POLL: Duration = Duration::from_secs(5);

/// Exits once no request is in flight and the broker has been idle past `limit` or its runtime was replaced.
pub fn when_idle(broker: &Arc<Broker>, socket: PathBuf, limit: Duration) {
    let watched = Arc::clone(broker);
    let mut runtime = north_runtime_follow::Follow::of(followed(north_runtime_follow::link()), std::env::current_exe().ok());
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

/// The link the broker follows when its starter followed `link`: that one, unless it is named for the runtime it
/// points at, as the link a canary judges a runtime through is. Such a link never moves, so a broker a canary's
/// probe started served that runtime long after the host promoted past it; it follows the host's `current` instead.
fn followed(link: PathBuf) -> PathBuf {
    let fixed = link.file_name().and_then(|name| name.to_str()).is_some_and(|name| north_runtime_follow::named(&link).as_deref() == Some(name));
    if fixed { north_runtime_follow::host_link() } else { link }
}
