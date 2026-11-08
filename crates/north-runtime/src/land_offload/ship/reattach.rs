use std::time::{Duration, Instant};

/// How long the runner keeps trying to read a run's result back after losing its connection, shorter than the minutes the run waits for a client before stopping itself.
pub(super) const WINDOW: Duration = Duration::from_secs(150);
pub(super) const PAUSE: Duration = Duration::from_secs(2);

/// The exit and reported status of a run whose connection ended, once `again` has read the run's result back from the host: a run that started and whose ssh exited 255 without its status is asked for again, each time through a fresh connection, until it answers, its host refuses it for another reason, or `window` passes.
pub(super) fn ride_out(first: (Option<u8>, Option<u8>), started: bool, window: Duration, pause: Duration, mut again: impl FnMut() -> (Option<u8>, Option<u8>)) -> (Option<u8>, Option<u8>) {
    let began = Instant::now();
    let mut last = first;
    while started && last.1.is_none() && last.0 == Some(super::TRANSPORT_FAILED) && began.elapsed() < window {
        std::thread::sleep(pause);
        last = again();
    }
    last
}
