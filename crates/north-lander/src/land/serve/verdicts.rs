use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use north_base::land_ticket;
use rustix::fs::inotify::{self, CreateFlags, Reader, WatchFlags};
use rustix::io::Errno;

use super::super::lander::Lander;

pub struct Watcher {
    stop: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sends each verdict the lander writes to the outbox host its ticket came from, as the decision lands in a decided area.
pub fn watch(lander: &Lander) -> Watcher {
    let stop = Arc::new(AtomicBool::new(false));
    let areas: Vec<PathBuf> = land_ticket::directories(&lander.queue).into_iter().skip(1).collect();
    let started = inotify::init(CreateFlags::CLOEXEC).map_err(|error| error.to_string()).and_then(|events| {
        let watched = areas
            .into_iter()
            .map(|area| {
                std::fs::create_dir_all(&area).map_err(|error| format!("{}: {error}", area.display()))?;
                inotify::add_watch(&events, &area, WatchFlags::MOVED_TO).map(|descriptor| (descriptor, area)).map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let (stopping, owned) = (Arc::clone(&stop), lander.detached());
        std::thread::Builder::new().name("land verdicts".into()).spawn(move || follow(&events, &watched, &owned, &stopping)).map_err(|error| error.to_string())
    });
    if let Err(error) = started {
        println!("north land: could not start sending verdicts to the outbox hosts, so a hand-in there reads them from the records mirror: {error}");
    }
    Watcher { stop }
}

fn follow(events: &impl std::os::fd::AsFd, watched: &[(i32, PathBuf)], lander: &Lander, stop: &AtomicBool) {
    let mut buffer = [MaybeUninit::<u8>::uninit(); 4096];
    let mut reader = Reader::new(events, &mut buffer);
    while !stop.load(Ordering::Relaxed) {
        let event = match reader.next() {
            Ok(event) => event,
            Err(Errno::INTR) => continue,
            Err(error) => {
                println!("north land: stopped sending verdicts to the outbox hosts, so a hand-in there reads them from the records mirror: {error}");
                return;
            }
        };
        let Some(name) = event.file_name().and_then(|name| name.to_str().ok()).filter(|name| name.ends_with(".json") && !name.starts_with('.')) else { continue };
        let Some((_, area)) = watched.iter().find(|(descriptor, _)| *descriptor == event.wd()) else { continue };
        match lander.deliver(&area.join(name)) {
            Ok(Some(host)) => println!("north land: sent the verdict on {name} to {host}"),
            Ok(None) => {}
            Err(error) => println!("north land: could not send the verdict on {name} to its outbox host, so its hand-in asks for it or reads the records mirror: {error}"),
        }
    }
}
