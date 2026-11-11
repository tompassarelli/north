use std::collections::{HashMap, HashSet};
use std::thread::JoinHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::super::lander::{Lander, plain};

const WAIT: Duration = Duration::from_secs(10);
const REST: Duration = Duration::from_secs(5);
const DISCOVERY: Duration = Duration::from_secs(30);
const STEP: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Wake {
    rung: Mutex<bool>,
    bell: Condvar,
}

impl Wake {
    pub fn ring(&self) {
        if let Ok(mut rung) = self.rung.lock() {
            *rung = true;
            self.bell.notify_all();
        }
    }

    pub fn wait(&self, timeout: Duration) -> bool {
        let Ok(rung) = self.rung.lock() else {
            std::thread::sleep(timeout);
            return false;
        };
        self.bell.wait_timeout_while(rung, timeout, |rung| !*rung).is_ok_and(|(mut rung, _)| std::mem::take(&mut *rung))
    }
}

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
            if live.get(&host).is_some_and(|follower| !follower.is_finished()) {
                continue;
            }
            let (lander, stop, wake, named) = (Arc::clone(lander), Arc::clone(stop), Arc::clone(wake), host.clone());
            match std::thread::Builder::new().name(format!("land outbox {host}")).spawn(move || follow(&lander, &named, &stop, &wake)) {
                Ok(follower) => {
                    live.insert(host, follower);
                }
                Err(error) => println!("north land: could not start watching {host}'s outbox: {error}"),
            }
        }
        pause(DISCOVERY, stop);
    }
}

fn follow(lander: &Lander, host: &str, stop: &AtomicBool, wake: &Wake) {
    while !stop.load(Ordering::Relaxed) {
        match lander.pull_host(host, WAIT) {
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
        if !lander.remote_hosts().iter().any(|listed| listed == host) {
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
