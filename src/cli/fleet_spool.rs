use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::relay::hostname;
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::store::{FleetRow, JOB_LEASE_SECS, JobPayload, RowKind};

const USAGE: &str = "Usage: north fleet spool take | north fleet spool drop ID... | north fleet spool job start SESSION KEY TEXT | north fleet spool job beat SESSION KEY | north fleet spool job beats SESSION KEY EVERY_SECS PID | north fleet spool job exit SESSION KEY CODE TEXT";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let spool = fleet.spool();
    match words {
        ["take"] => println!("{}", serde_json::to_string(&spool.pending()?)?),
        ["drop", ids @ ..] if !ids.is_empty() => spool.withdraw_all(&ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>())?,
        ["job", "start", session, key, text] => spool_job(fleet, RowKind::JobStart, session, key, None, text).map(drop)?,
        ["job", "beat", session, key] => spool_job(fleet, RowKind::JobBeat, session, key, None, "").map(drop)?,
        ["job", "beats", session, key, every, pid] => beats(fleet, session, key, parse(every)?, parse(pid)?)?,
        ["job", "exit", session, key, code, text] => exit(fleet, session, key, parse(code)?, text)?,
        _ => return Err(NorthError::Usage(USAGE.into())),
    }
    Ok(())
}

fn parse<T: std::str::FromStr>(text: &str) -> NorthResult<T> {
    text.parse().map_err(|_| NorthError::Usage(format!("{text:?} is not a number; {USAGE}")))
}

fn spool_job(fleet: &Fleet, kind: RowKind, session: &str, key: &str, exit: Option<i32>, text: &str) -> NorthResult<FleetRow> {
    let sent_at = now_ms();
    let lease_secs = (kind != RowKind::JobExit).then_some(JOB_LEASE_SECS);
    let row = FleetRow {
        id: format!("{sent_at}-{}", new_uuid()?),
        kind,
        to: session.into(),
        from: hostname(),
        urgent: false,
        sent_at,
        text: text.into(),
        sender: None,
        gate: None,
        job: Some(JobPayload { key: key.into(), lease_secs, exit }),
    };
    fleet.spool().put(&row)?;
    Ok(row)
}

fn beats(fleet: &Fleet, session: &str, key: &str, every: u64, pid: u32) -> NorthResult<()> {
    let running = Path::new("/proc").join(pid.to_string());
    loop {
        sleep(Duration::from_secs(every));
        if !running.exists() {
            return Ok(());
        }
        spool_job(fleet, RowKind::JobBeat, session, key, None, "").map(drop)?;
    }
}

fn exit(fleet: &Fleet, session: &str, key: &str, code: i32, text: &str) -> NorthResult<()> {
    let row = spool_job(fleet, RowKind::JobExit, session, key, Some(code), text)?;
    let carried = if fleet.session(session).is_ok() { fleet.hand_in(&row).map(drop) } else { fleet.outbox().post(&row).map(drop) };
    if let Err(error) = carried {
        eprintln!("north fleet: job exit {} stays spooled for the next tick: {}", row.id, error.user_message());
    }
    Ok(())
}
