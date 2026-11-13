use std::collections::{HashMap, HashSet};
use std::thread::JoinHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::after::route::{self, Outbox};
use super::super::lander::{Lander, plain};
use super::super::wake::Wake;

const WAIT: Duration = Duration::from_secs(10);
const REST: Duration = Duration::from_secs(5);
const DISCOVERY: Duration = Duration::from_secs(30);
const STEP: Duration = Duration::from_millis(250);

pub struct Watchers {
    stop: Arc<AtomicBool>,
}

impl Drop for Watchers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn watch(lander: &Lander, wake: &Arc<Wake>) -> Watchers {
    let stop = Arc::new(AtomicBool::new(false));
    let owned = lander.detached();
    let (stopping, waking) = (Arc::clone(&stop), Arc::clone(wake));
    if let Err(error) = std::thread::Builder::new().name("land outbox hosts".into()).spawn(move || supervise(&Arc::new(owned), &stopping, &waking)) {
        println!("north land: could not start watching the build hosts' outboxes: {error}");
    }
    Watchers { stop }
}

fn supervise(lander: &Arc<Lander>, stop: &Arc<AtomicBool>, wake: &Arc<Wake>) {
    let mut live: HashMap<String, JoinHandle<()>> = HashMap::new();
    let mut skipped = HashSet::new();
    while !stop.load(Ordering::Relaxed) {
        for host in lander.remote_hosts() {
            if !plain(&host) {
                if skipped.insert(host.clone()) {
                    println!("north land: waiting: the outbox hosts name {host:?}, which is not an ssh host name; skipped");
                }
                continue;
            }
            let outboxes = match route::outboxes(&host) {
                Ok(outboxes) => outboxes,
                Err(error) => {
                    if skipped.insert(host.clone()) {
                        println!("north land: waiting: {host}'s outbox: {error}");
                    }
                    continue;
                }
            };
            for outbox in outboxes {
                start(lander, outbox, &mut live, stop, wake);
            }
        }
        pause(DISCOVERY, stop);
    }
}

fn start(lander: &Arc<Lander>, outbox: Outbox, live: &mut HashMap<String, JoinHandle<()>>, stop: &Arc<AtomicBool>, wake: &Arc<Wake>) {
    let named = outbox.name().to_owned();
    if live.get(&named).is_some_and(|follower| !follower.is_finished()) {
        return;
    }
    let (lander, stop, wake) = (Arc::clone(lander), Arc::clone(stop), Arc::clone(wake));
    match std::thread::Builder::new().name(format!("land outbox {named}")).spawn(move || follow(&lander, &outbox, &stop, &wake)) {
        Ok(follower) => {
            live.insert(named, follower);
        }
        Err(error) => println!("north land: could not start watching {named}'s outbox: {error}"),
    }
}

fn follow(lander: &Lander, outbox: &Outbox, stop: &AtomicBool, wake: &Wake) {
    let host = outbox.name();
    let mut taken = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match lander.pull_host(outbox, WAIT, &mut taken) {
            Ok(0) => {}
            Ok(pulled) => {
                println!("north land: pulled {pulled} tickets from {host}'s outbox");
                wake.ring();
            }
            Err(error) => {
                println!("north land: waiting: {host}'s outbox: {error}");
                pause(REST, stop);
            }
        }
        if !lander.remote_hosts().contains(&outbox.host) || !route::outboxes(&outbox.host).is_ok_and(|outboxes| outboxes.contains(outbox)) {
            return;
        }
    }
}

fn pause(length: Duration, stop: &AtomicBool) {
    let until = Instant::now() + length;
    while Instant::now() < until && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(STEP);
    }
}
