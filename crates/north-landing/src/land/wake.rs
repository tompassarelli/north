use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

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

    pub fn ring_on_exit(self: &Arc<Self>, pid: u32) {
        let wake = Arc::clone(self);
        let pidfd = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw).and_then(|pid| rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).ok());
        let Some(pidfd) = pidfd else {
            wake.ring();
            return;
        };
        let watched = std::thread::Builder::new().name("land check exit".into()).spawn(move || {
            let mut exited = [rustix::event::PollFd::new(&pidfd, rustix::event::PollFlags::IN)];
            while matches!(rustix::event::poll(&mut exited, -1), Err(rustix::io::Errno::INTR)) {}
            wake.ring();
        });
        if let Err(error) = watched {
            println!("north land: could not watch check {pid} for its exit, so the lander notices it on its backstop tick: {error}");
        }
    }
}

pub struct Task<T> {
    done: Arc<AtomicBool>,
    handle: JoinHandle<T>,
}

impl<T: Send + 'static> Task<T> {
    pub fn spawn(wake: &Arc<Wake>, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (done, rung) = (Arc::new(AtomicBool::new(false)), Arc::clone(wake));
        let finished = Arc::clone(&done);
        let handle = std::thread::spawn(move || ringing(&finished, &rung, work));
        Self { done, handle }
    }

    pub fn named(name: String, wake: &Arc<Wake>, work: impl FnOnce() -> T + Send + 'static) -> std::io::Result<Self> {
        let (done, rung) = (Arc::new(AtomicBool::new(false)), Arc::clone(wake));
        let finished = Arc::clone(&done);
        let handle = std::thread::Builder::new().name(name).spawn(move || ringing(&finished, &rung, work))?;
        Ok(Self { done, handle })
    }

    pub fn is_finished(&self) -> bool {
        self.done.load(Ordering::Acquire) || self.handle.is_finished()
    }

    pub fn join(self) -> std::thread::Result<T> {
        self.handle.join()
    }
}

fn ringing<T>(done: &AtomicBool, wake: &Wake, work: impl FnOnce() -> T) -> T {
    let result = work();
    done.store(true, Ordering::Release);
    wake.ring();
    result
}
