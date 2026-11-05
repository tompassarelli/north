use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::broker::Broker;
use super::calls::CallLog;
use super::http::{Response, read_request, write_response};

#[derive(Default)]
pub struct Activity {
    open: AtomicUsize,
    last: AtomicU64,
}

impl Activity {
    fn touch(&self) {
        self.last.store(unix_now(), Ordering::Relaxed);
    }

    pub fn idle_for(&self) -> Option<Duration> {
        (self.open.load(Ordering::Relaxed) == 0).then(|| Duration::from_secs(unix_now().saturating_sub(self.last.load(Ordering::Relaxed))))
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub fn serve(listener: &UnixListener, broker: &Arc<Broker>, activity: &Arc<Activity>, log: &Arc<CallLog>) {
    activity.touch();
    for stream in listener.incoming().flatten() {
        activity.open.fetch_add(1, Ordering::Relaxed);
        activity.touch();
        let (broker, activity, log) = (Arc::clone(broker), Arc::clone(activity), Arc::clone(log));
        std::thread::spawn(move || {
            answer(stream, &broker, &log);
            activity.touch();
            activity.open.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

fn answer(stream: UnixStream, broker: &Broker, log: &CallLog) {
    let (response, source) = match read_request(&stream) {
        Ok(request) => {
            let (response, source) = broker.handle(&request);
            log.record(&stream, &request, &response, source);
            (response, source.name())
        }
        Err(error) => (Response::json(400, Vec::new(), &format!("north gh-broker could not read the request: {error}")), "refused"),
    };
    let _ = write_response(&stream, &response, source);
}
