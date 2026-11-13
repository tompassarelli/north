//! Each host's deploy deadline, from the p90 of its measured ships, and a wait that stops at each host's deadline, so a host that never answers holds back no other host and no later promotion: from 12:40 to 12:58 PM on 2026-11-05 one promote waited on saturated gex45 while 15 more stacked behind it (north#1247).

use super::layout::Layout;
use super::pin::{self, Event};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const WINDOW: i64 = 7 * 24 * 3600;
const MARGIN: u32 = 2;
const FLOOR: Duration = Duration::from_secs(120);
const UNMEASURED: Duration = Duration::from_secs(10 * 60);

/// How long a deploy to `host` may take before it is skipped until the next tick.
pub(super) fn of(layout: &Layout, host: &str) -> Duration {
    measured(&pin::read(&layout.state), host, pin::now()).unwrap_or(UNMEASURED)
}

fn measured(events: &[Event], host: &str, now: i64) -> Option<Duration> {
    let mut seconds: Vec<u64> = events.iter().filter_map(|event| match event {
        Event::Shipped { host: shipped, at, seconds, .. } if shipped == host && now - at <= WINDOW => u64::try_from(*seconds).ok(),
        _ => None,
    }).collect();
    seconds.sort_unstable();
    let p90 = *seconds.get((seconds.len() * 9).div_ceil(10).checked_sub(1)?)?;
    Some((Duration::from_secs(p90) * MARGIN).max(FLOOR))
}

pub(super) type Job<T> = (String, Duration, Box<dyn FnOnce() -> T + Send>);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reached<T> {
    Answered(T),
    Skipped(Duration),
}

/// Runs every host's job on a thread of its own and returns once each has answered or passed its deadline; a job past its deadline is left running and named skipped.
pub(super) fn each_within<T: Send + 'static>(jobs: Vec<Job<T>>) -> Vec<(String, Reached<T>)> {
    let started = Instant::now();
    let (sent, answers) = mpsc::channel();
    let mut hosts = Vec::new();
    for (index, (host, deadline, job)) in jobs.into_iter().enumerate() {
        let sent = sent.clone();
        std::thread::spawn(move || {
            let _ = sent.send((index, job()));
        });
        hosts.push((host, deadline, None));
    }
    drop(sent);
    while let Some(next) = hosts.iter().filter(|(_, _, reached)| reached.is_none()).map(|(_, deadline, _)| *deadline).min() {
        match answers.recv_timeout(next.saturating_sub(started.elapsed())) {
            Ok((index, answer)) => hosts[index].2 = Some(Reached::Answered(answer)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                for (_, deadline, reached) in hosts.iter_mut().filter(|(_, deadline, reached)| reached.is_none() && *deadline <= started.elapsed()) {
                    *reached = Some(Reached::Skipped(*deadline));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    hosts.into_iter().map(|(host, deadline, reached)| (host, reached.unwrap_or(Reached::Skipped(deadline)))).collect()
}

