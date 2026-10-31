use std::io::Read;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::store::{FleetRow, JOB_LEASE_SECS, JobPayload, RowKind, SessionRow};
use crate::fleet::tick;

const DELIVER_BUDGET: Duration = Duration::from_secs(25);

const USAGE: &str = "Usage: north fleet spool take | north fleet spool drop < IDS | north fleet spool set-aside < IDS | north fleet spool cache < SESSIONS_JSON | north fleet spool registry | north fleet spool deliver < ROWS_JSON | north fleet spool job start SESSION KEY TEXT | north fleet spool job beat SESSION KEY | north fleet spool job beats SESSION KEY EVERY_SECS PID | north fleet spool job exit SESSION KEY CODE TEXT";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let spool = fleet.spool();
    match words {
        ["take"] => println!("{}", serde_json::to_string(&spool.listing()?)?),
        ["drop"] => spool.withdraw_all(&ids_on_stdin()?)?,
        ["set-aside"] => spool.set_aside(&ids_on_stdin()?)?,
        ["cache"] => fleet.cache_sessions(&serde_json::from_reader::<_, Vec<SessionRow>>(std::io::stdin())?)?,
        ["registry"] => println!("{}", serde_json::to_string(&tick::registry(fleet, &fleet.sessions()?, now_ms())?)?),
        ["deliver"] => println!("{}", serde_json::to_string(&tick::hand_in_all(fleet, &serde_json::from_reader::<_, Vec<FleetRow>>(std::io::stdin())?, Instant::now(), DELIVER_BUDGET))?),
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
        from: fleet.host().to_owned(),
        urgent: false,
        sent_at,
        text: text.into(),
        sender: None,
        gate: None,
        answer: None,
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
    if fleet.session(session).is_ok()
        && let Err(error) = fleet.deliver_row(&row)
    {
        eprintln!("north fleet: job exit {} stays spooled for the next tick: {}", row.id, error.user_message());
    }
    Ok(())
}

fn ids_on_stdin() -> NorthResult<Vec<String>> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    Ok(text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
}
