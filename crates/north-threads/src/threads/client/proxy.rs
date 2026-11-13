use std::fs;
use std::io::{Read, Write};
use std::time::Duration;

use super::process::{Input, agent_event_checks, has_usage};
use super::{Client, Flush};
use crate::error::NorthResult;
use crate::foundation::exit::{TIMED_OUT_STATUS, TRANSPORT_FAILED_STATUS};
use crate::threads::ingest_lock;

const READS: [&str; 11] = [
    "job-runs", "show", "ready", "list", "holder", "unowned", "run-ids", "summary", "recommend", "calibrate", "opencode",
];

/// How old a snapshot may be before an ingest carrying usage pulls it: each pull rebuilds the whole ledger image on the ledger host, and every host ingests usage each minute.
const USAGE_SNAPSHOT_AGE: Duration = Duration::from_secs(300);

fn unanswered(code: i32) -> &'static str {
    if code == TIMED_OUT_STATUS { "ledger host timed out" } else { "ledger host unreachable" }
}

fn keyed_agent_event(argv: &[String], err: &mut dyn Write) -> NorthResult<Option<Vec<String>>> {
    if let Err(message) = agent_event_checks(argv) {
        writeln!(err, "NorthThreads: {message}")?;
        return Ok(None);
    }
    let mut keyed = argv.to_vec();
    if !argv.iter().any(|arg| arg == "--key" || arg.starts_with("--key=")) {
        let key = fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_owned();
        keyed.extend(["--key".to_owned(), key]);
    }
    Ok(Some(keyed))
}

impl Client {
    pub(super) fn proxy(&self, dest: &str, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<Option<i32>> {
        let first = argv.first().map(String::as_str);
        let mut argv = argv.to_vec();
        if first == Some("agent-event") {
            let Some(keyed) = keyed_agent_event(&argv, err)? else { return Ok(Some(1)) };
            argv = keyed;
        }
        if first == Some("job-run") {
            return self.proxy_job_run(dest, &argv, err).map(Some);
        }
        let flush = self.flush_spool_for(first, dest, err)?;
        if first == Some("ingest") {
            return self.proxy_ingest(dest, &argv, stdin, flush, err).map(Some);
        }
        if first == Some("agent-event") {
            return self.proxy_agent_event(dest, &argv, flush, err).map(Some);
        }
        let reachable = flush != Flush::Down;
        let follow = first == Some("queue") && argv.get(1).map(String::as_str) == Some("follow");
        let input = if follow {
            let mut brief = Vec::new();
            stdin.read_to_end(&mut brief)?;
            Input::Bytes(brief)
        } else {
            Input::Inherit
        };
        let code = if reachable { self.ssh(dest, &argv, input, false, err)? } else { TRANSPORT_FAILED_STATUS };
        if first == Some("agent-events") || first == Some("agent-event-state") {
            if code == TRANSPORT_FAILED_STATUS {
                writeln!(err, "NorthThreads: {dest} unreachable; canonical events unavailable")?;
            }
            return Ok(Some(code));
        }
        if code == TIMED_OUT_STATUS {
            return Ok(Some(code));
        }
        if code != TRANSPORT_FAILED_STATUS {
            self.refresh_after_answer(dest, &argv, code)?;
            return Ok(Some(code));
        }
        if first.is_some_and(|verb| READS.contains(&verb)) && self.snapshot.exists() {
            writeln!(err, "{}", self.stale_line(dest)?)?;
            return Ok(None);
        }
        writeln!(err, "NorthThreads: {dest} unreachable; the ledger lives there, so nothing was read or recorded")?;
        Ok(Some(code))
    }

    fn proxy_job_run(&self, dest: &str, argv: &[String], err: &mut dyn Write) -> NorthResult<i32> {
        let code = self.ssh(dest, argv, Input::Null, false, err)?;
        self.spool_if_unanswered(argv, code)
    }

    fn proxy_ingest(&self, dest: &str, argv: &[String], stdin: &mut dyn Read, flush: Flush, err: &mut dyn Write) -> NorthResult<i32> {
        let mut data = Vec::new();
        stdin.read_to_end(&mut data)?;
        let mut state = if flush != Flush::Down { "busy" } else { "unreachable" };
        if flush == Flush::Clear {
            let code = self.ssh(dest, argv, Input::Bytes(data.clone()), false, err)?;
            if code == ingest_lock::RETRY {
                state = "busy";
            } else if code == TIMED_OUT_STATUS {
                state = "timed out";
            } else if code != TRANSPORT_FAILED_STATUS {
                if code == 0 && has_usage(&data)? {
                    self.refresh_snapshot_later(dest, USAGE_SNAPSHOT_AGE);
                }
                return Ok(code);
            }
        }
        let path = self.write_spool(&data, &format!("ledger host {state}"))?;
        writeln!(err, "NorthThreads: {dest} {state}; spooled ingest to {}", path.display())?;
        Ok(0)
    }

    fn proxy_agent_event(&self, dest: &str, argv: &[String], flush: Flush, err: &mut dyn Write) -> NorthResult<i32> {
        let code = if flush == Flush::Clear { self.ssh(dest, argv, Input::Null, false, err)? } else { TRANSPORT_FAILED_STATUS };
        self.spool_if_unanswered(argv, code)
    }

    fn spool_if_unanswered(&self, argv: &[String], code: i32) -> NorthResult<i32> {
        if !matches!(code, TRANSPORT_FAILED_STATUS | TIMED_OUT_STATUS) {
            return Ok(code);
        }
        self.spool_argv(argv, unanswered(code))?;
        Ok(0)
    }
}
