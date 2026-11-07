use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

pub fn after(span: Duration) -> Instant {
    Instant::now() + span
}

pub fn until(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = after(limit);
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

pub async fn until_async(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = after(limit);
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}

pub fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let result = work();
    (result, started.elapsed())
}

pub fn hang() {
    loop {
        std::thread::park();
    }
}
