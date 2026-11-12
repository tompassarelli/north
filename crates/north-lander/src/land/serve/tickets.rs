use std::collections::HashSet;
use std::mem::MaybeUninit;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::fs::inotify::{self, CreateFlags, ReadFlags, Reader, WatchFlags};
use rustix::io::Errno;

use super::super::wake::Wake;

pub struct Watcher {
    stop: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn watch(queue: &Path, wake: &Arc<Wake>) -> Watcher {
    let stop = Arc::new(AtomicBool::new(false));
    let (stopping, waking, watched) = (Arc::clone(&stop), Arc::clone(wake), queue.to_owned());
    let started = inotify::init(CreateFlags::CLOEXEC)
        .and_then(|queue_events| inotify::add_watch(&queue_events, &watched, WatchFlags::MOVED_TO | WatchFlags::MOVED_FROM | WatchFlags::DELETE).map(|_| queue_events))
        .map_err(|error| error.to_string())
        .and_then(|queue_events| std::thread::Builder::new().name("land queue tickets".into()).spawn(move || follow(&queue_events, &stopping, &waking)).map_err(|error| error.to_string()));
    if let Err(error) = started {
        println!("north land: could not start watching {} for new tickets, so they wait for the next round: {error}", queue.display());
    }
    Watcher { stop }
}

fn follow(queue_events: &impl std::os::fd::AsFd, stop: &AtomicBool, wake: &Wake) {
    let mut buffer = [MaybeUninit::<u8>::uninit(); 4096];
    let mut reader = Reader::new(queue_events, &mut buffer);
    let mut queued: HashSet<String> = HashSet::new();
    while !stop.load(Ordering::Relaxed) {
        let event = match reader.next() {
            Ok(event) => event,
            Err(Errno::INTR) => continue,
            Err(error) => {
                println!("north land: stopped watching the queue for new tickets, so they wait for the next round: {error}");
                return;
            }
        };
        let Some(name) = event.file_name().and_then(|name| name.to_str().ok()).filter(|name| name.ends_with(".json") && !name.starts_with('.')) else { continue };
        if !event.events().contains(ReadFlags::MOVED_TO) {
            queued.remove(name);
        } else if queued.insert(name.to_owned()) {
            wake.ring();
        }
    }
}

#[cfg(test)]
#[path = "tickets_tests.rs"]
mod tests;
