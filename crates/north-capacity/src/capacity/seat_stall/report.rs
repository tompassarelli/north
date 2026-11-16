//! `north capacity seat-stall --since DURATION` prints each stall of the seat over 1 s from the probe's log, with its length, the fleet units running then and whether and when the breaker stopped them; `north capacity seat-stall resume` starts the units the breaker stopped, and refuses while a synced write is out past the trip time.

use std::process::{Command, Stdio};

use north_foundation::foundation::clock::Timestamp;

use super::{EVENTS, Event, Log, SLOW_MS, append, dir, now_ms, trip_ms};
use crate::capacity::window::parse_window_seconds;

const USAGE: &str = "usage: north capacity seat-stall --since DURATION | resume";
const STOP_WITHIN_MS: u64 = 10_000;

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [flag, since] if flag == "--since" => match parse_window_seconds(since) {
            Some(seconds) => report(seconds),
            None => {
                eprintln!("{USAGE}");
                2
            }
        },
        [verb] if verb == "resume" => resume(),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn read() -> Result<Log, String> {
    let dir = dir();
    Log::read(&dir)?.ok_or_else(|| format!("no seat probe log in {}; the seat-probe unit writes it", dir.display()))
}

fn report(seconds: f64) -> u8 {
    match read() {
        Ok(log) => {
            let now = now_ms();
            for line in lines(&log, now.saturating_sub((seconds * 1000.0) as u64), now, trip_ms()) {
                println!("{line}");
            }
            0
        }
        Err(error) => {
            eprintln!("north capacity seat-stall: {error}");
            1
        }
    }
}

fn clock(at_ms: u64) -> String {
    Timestamp::from_unix_millis(i64::try_from(at_ms).unwrap_or(i64::MAX)).iso()
}

fn lines(log: &Log, since: u64, now: u64, trip: u64) -> Vec<String> {
    let mut stalls: Vec<(u64, u64, &[String], &str)> = log.samples.iter().filter(|sample| sample.at_ms >= since && sample.sync_ms > SLOW_MS).map(|sample| (sample.at_ms, sample.sync_ms, sample.units.as_slice(), "")).collect();
    let pending = log.pending_ms(now);
    if pending > SLOW_MS {
        stalls.push((now - pending, pending, &[], ", still out"));
    }
    if stalls.is_empty() {
        return vec![format!("no stall of the seat over {:.0} s since {}", SLOW_MS as f64 / 1000.0, clock(since))];
    }
    stalls.into_iter().map(|(at, length, units, out)| describe(log, at, length, units, out, trip)).collect()
}

fn describe(log: &Log, at: u64, length: u64, units: &[String], out: &str, trip: u64) -> String {
    let stop = log.events.iter().find_map(|event| match event {
        Event::Stop { at_ms, stall_since_ms, units } if *stall_since_ms == at || (at..=at + STOP_WITHIN_MS).contains(at_ms) => Some((*at_ms, units.as_slice())),
        _ => None,
    });
    let then = if units.is_empty() { stop.map_or(&[][..], |(_, units)| units) } else { units };
    let then = if then.is_empty() { "none read".to_owned() } else { then.join(", ") };
    let breaker = match stop {
        Some((stopped, units)) => stopped_verdict(at, stopped, units),
        None => unstopped_verdict(log, at, length, units, trip),
    };
    format!("{} stall {:.1} s{out}; fleet units then: {then}; breaker: {breaker}", clock(at), length as f64 / 1000.0)
}

fn stopped_verdict(at: u64, stopped: u64, units: &[String]) -> String {
    let after = stopped.saturating_sub(at) as f64 / 1000.0;
    if units.is_empty() {
        format!("tripped at {} (+{after:.1} s) and found no fleet unit running", clock(stopped))
    } else {
        format!("stopped {} units at {} (+{after:.1} s)", units.len(), clock(stopped))
    }
}

fn unstopped_verdict(log: &Log, at: u64, length: u64, units: &[String], trip: u64) -> String {
    if length < trip {
        return format!("not tripped, under the {:.0} s trip", trip as f64 / 1000.0);
    }
    match log.events.iter().rev().find(|event| event.at_ms() <= at) {
        Some(Event::Stop { at_ms, units: stopped, .. }) if !stopped.is_empty() && units.is_empty() => format!("units already stopped since {}", clock(*at_ms)),
        _ => format!("NOT STOPPED within {} s", STOP_WITHIN_MS / 1000),
    }
}

fn resume() -> u8 {
    let log = match read() {
        Ok(log) => log,
        Err(error) => {
            eprintln!("north capacity seat-stall resume: {error}");
            return 1;
        }
    };
    let (now, trip) = (now_ms(), trip_ms());
    let pending = log.pending_ms(now);
    if pending > trip {
        eprintln!("north capacity seat-stall resume: refused: the seat probe's synced write has been out {:.1} s since {}, so a stall is in progress or the probe is not running; resume once a synced write returns", pending as f64 / 1000.0, clock(now - pending));
        return 1;
    }
    let Some(Event::Stop { units, at_ms, .. }) = log.standing_stop() else {
        println!("north capacity seat-stall resume: the breaker holds no unit stopped");
        return 0;
    };
    match Command::new("systemctl").args(["--user", "start", "--"]).args(units).stdin(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            eprintln!("north capacity seat-stall resume: systemctl start exited {status}");
            return 1;
        }
        Err(error) => {
            eprintln!("north capacity seat-stall resume: systemctl start: {error}");
            return 1;
        }
    }
    let resumed = serde_json::to_string(&Event::Resume { at_ms: now_ms(), units: units.clone() }).map_err(|error| error.to_string()).and_then(|line| append(&dir(), EVENTS, &line));
    if let Err(error) = resumed {
        eprintln!("north capacity seat-stall resume: the units started, but the resume was not recorded: {error}");
        return 1;
    }
    println!("north capacity seat-stall resume: started the {} units the breaker stopped at {}: {}", units.len(), clock(*at_ms), units.join(", "));
    0
}
