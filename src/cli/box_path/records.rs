use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::lifecycle::Store;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;

use super::{BoxPath, Mark, Phase};
use crate::error::{NorthError, NorthResult};
use crate::fleet::audience::fleet_dir;
use crate::fleet::hops::{Launch, launches, trace_in};
use crate::foundation::clock::Timestamp;
use crate::land::queue::{self, Ticket};
use crate::land::trace::{Span, Spans};
use crate::platform::{land_queue, records_mirror};
use crate::threads::claims::lifecycle::Claiming;
use crate::threads::claims::{box_slug, same_box};

const RUNTIME_REPOSITORY: &str = "north";
const NEAR_MS: i64 = 120_000;

struct Run {
    agent: String,
    session: String,
    slug: Option<String>,
    started_ms: Option<i64>,
    ended_ms: Option<i64>,
    outcome: String,
    landed_sha: String,
    landed_ms: Option<i64>,
}

struct Found {
    path: PathBuf,
    queue: PathBuf,
    ticket: Ticket,
}

fn ms(text: &str) -> Option<i64> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_millis)
}

fn sql(error: rusqlite::Error) -> NorthError {
    NorthError::Usage(format!("north box path: the threads ledger: {error}"))
}

fn read_only(path: &Path) -> Option<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

pub(super) fn paths(item: &str, wanted: Option<&str>) -> NorthResult<Vec<BoxPath>> {
    let threads = crate::ledger::threads_database();
    let db = read_only(&threads).ok_or_else(|| NorthError::Usage(format!("north box path: cannot open the threads ledger {}", threads.display())))?;
    let runs = runs(&db, item).map_err(sql)?;
    let wanted = wanted.and_then(box_slug);
    let mut tries: Vec<&Run> = Vec::new();
    for run in &runs {
        let matches = match (&wanted, &run.slug) {
            (Some(wanted), Some(slug)) => same_box(wanted, slug),
            (Some(_), None) => false,
            (None, _) => true,
        };
        if !matches {
            continue;
        }
        let same = tries.iter().position(|kept| kept.slug.as_deref().zip(run.slug.as_deref()).map_or(kept.slug == run.slug, |(left, right)| same_box(left, right)));
        match same {
            Some(index) if !tries[index].landed_sha.is_empty() && run.landed_sha.is_empty() => {}
            Some(index) => tries[index] = run,
            None => tries.push(run),
        }
    }
    let claims = Store::open(&threads).ok();
    let records = records_mirror::read("north box path");
    Ok(tries.into_iter().map(|run| path(item, run, &db, claims.as_ref(), records.home())).collect())
}

fn runs(db: &Connection, item: &str) -> rusqlite::Result<Vec<Run>> {
    let mut statement = db.prepare(
        "SELECT fleet_name, coalesce(box, ''), coalesce(started, ''), ended, outcome, coalesce(landed_sha, ''), coalesce(landed_at, ''), agent FROM runs WHERE item = ?1 AND coalesce(fleet_name, '') != '' ORDER BY started",
    )?;
    let rows = statement.query_map([item], |row| {
        let (started, ended, landed): (String, String, String) = (row.get(2)?, row.get(3)?, row.get(6)?);
        Ok(Run { agent: row.get(7)?, session: row.get(0)?, slug: box_slug(&row.get::<_, String>(1)?), started_ms: ms(&started), ended_ms: ms(&ended), outcome: row.get(4)?, landed_sha: row.get(5)?, landed_ms: ms(&landed) })
    })?;
    rows.collect()
}

fn path(item: &str, run: &Run, db: &Connection, claims: Option<&Store>, records: &Path) -> BoxPath {
    let mut path = BoxPath { title: format!("{item}{} worked by {}", run.slug.as_ref().map(|slug| format!(" box {slug}")).unwrap_or_default(), run.session), marks: Vec::new(), missing: Vec::new() };
    let near = run.started_ms.unwrap_or(i64::MAX);
    let mut mark = |phase: Phase, found: Result<(i64, String), String>| match found {
        Ok((at_ms, source)) => path.marks.push(Mark { phase, at_ms, source }),
        Err(why) => path.missing.push((phase, why)),
    };
    let claimed = claims.ok_or_else(|| "the threads ledger's claim store cannot be opened".to_owned()).and_then(|store| claim(store, item, run));
    let floor = claimed.as_ref().map_or(near - 3_600_000, |(at, _)| *at - NEAR_MS);
    mark(Phase::Claimed, claimed);
    let launch = launch(&run.session, near);
    match &launch {
        Some(launch) => {
            mark(Phase::Launching, Ok((millis(launch.registered_ms), format!("session {} registered (launch spans, hops.jsonl)", run.session))));
            mark(Phase::Working, launch.response_ms.map(|at| (millis(at), "first answer (launch spans, hops.jsonl)".to_owned())).ok_or_else(|| "its launch spans hold no first answer".to_owned()));
        }
        None => {
            mark(Phase::Launching, placed(&run.agent, &run.session).or_else(|| registered(db, &run.session, floor, near)).ok_or_else(|| format!("no fleet session record of {} at this try", run.session)));
            mark(Phase::Working, run.started_ms.map(|at| (at, "the run's first line (threads runs; no launch spans on this host)".to_owned())).ok_or_else(|| "the run has no start".to_owned()));
        }
    }
    let found = ticket(records, &run.session, &run.landed_sha, floor);
    let landed_sha = found.as_ref().and_then(|found| found.ticket.verdict.as_deref()?.strip_prefix("landed ").map(str::to_owned)).unwrap_or_else(|| run.landed_sha.clone());
    let landed_ms = match &found {
        Some(found) => {
            let stem = found.path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            mark(Phase::HandingIn, found.ticket.hand_in.map(|hand_in| (micros(hand_in.called_micros), format!("hand-in called (land ticket {stem})"))).ok_or_else(|| "the ticket holds no hand-in".to_owned()));
            let queued = found.ticket.queued_micros.map(micros).or_else(|| ms(&found.ticket.queued_at));
            mark(Phase::Queued, queued.map(|at| (at, format!("ticket queued (land ticket {stem})"))).ok_or_else(|| "the ticket has no queued time".to_owned()));
            let (checking, landed) = landing_spans(&found.queue, &stem);
            mark(Phase::Checking, checking.map(|at| (at, format!("the lander took it (landing spans, landing:{stem})"))).ok_or_else(|| "no landing span of the ticket".to_owned()));
            let landed_ms = landed.or(run.landed_ms);
            let source = if landed.is_some() { format!("pushed {} (landing spans, landing:{stem})", short(&landed_sha)) } else { "landed_at (threads runs)".to_owned() };
            mark(Phase::Landed, landed_ms.map(|at| (at, source)).ok_or_else(|| format!("the ticket's verdict is {}", found.ticket.verdict.as_deref().unwrap_or("not yet given"))));
            landed_ms
        }
        None => {
            for phase in [Phase::HandingIn, Phase::Queued, Phase::Checking] {
                mark(phase, Err(format!("no land ticket of {} in this host's land queues or their records mirror", run.session)));
            }
            mark(Phase::Landed, run.landed_ms.map(|at| (at, format!("landed {} (threads runs)", short(&run.landed_sha)))).ok_or_else(|| "no landing recorded".to_owned()));
            run.landed_ms
        }
    };
    let reported = run.ended_ms.filter(|_| run.outcome != "running");
    mark(Phase::Reported, reported.map(|at| (at, format!("final turn, outcome {} (threads runs)", run.outcome))).ok_or_else(|| "the run has not ended".to_owned()));
    mark(Phase::Delivered, receipt(&run.session, reported.unwrap_or(floor)).ok_or_else(|| format!("no report receipt of {} in this host's run ledger", run.session)));
    if landed_sha.is_empty() {
        return path;
    }
    mark(Phase::Verified, after_landing(records, &landed_sha).ok_or_else(|| format!("no after-landing run of {} in post-land.tsv", short(&landed_sha))));
    if item.split('#').next() == Some(RUNTIME_REPOSITORY) {
        mark(Phase::Promoted, promoted(records, item, &landed_sha, landed_ms.unwrap_or(floor)).ok_or_else(|| format!("no runtime record holds {} yet", short(&landed_sha))));
    }
    path
}

fn millis(at: u64) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX)
}

fn micros(at: u128) -> i64 {
    i64::try_from(at / 1000).unwrap_or(i64::MAX)
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(10)]
}

fn claim(store: &Store, item: &str, run: &Run) -> Result<(i64, String), String> {
    let instances = store.instances_under::<Claiming>(item).map_err(|error| format!("the claim store: {error}"))?;
    let cause = format!("claimed by {}", run.session);
    let near = run.started_ms.unwrap_or(i64::MAX) + NEAR_MS;
    let mut best: Option<(i64, String)> = None;
    for record in instances {
        let (key, slug): (&str, Option<String>) = record.instance.split_once('@').map_or((record.instance.as_str(), None), |(key, text)| (key, box_slug(text)));
        let boxed = match (&slug, &run.slug) {
            (Some(left), Some(right)) => same_box(left, right),
            (None, _) => true,
            (Some(_), None) => false,
        };
        if key != item || !boxed {
            continue;
        }
        for logged in store.history("claim", &record.instance).unwrap_or_default() {
            let at = logged.at.unix_millis();
            if logged.to == "held" && !logged.refused && logged.cause == cause && at <= near && best.as_ref().is_none_or(|(kept, _)| at > *kept) {
                best = Some((at, format!("claim {} (threads ledger, claim moves)", record.instance)));
            }
        }
    }
    best.ok_or_else(|| format!("no claim by {} in the threads ledger's claim moves", run.session))
}

fn launch(session: &str, near: i64) -> Option<Launch> {
    let fleet = fleet_dir();
    let prefix = format!("{session}-");
    let archived = fs::read_dir(fleet.join(".archive")).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|dir| dir.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix(&prefix)).is_some_and(|stamp| stamp.chars().all(|c| c.is_ascii_digit())));
    std::iter::once(fleet.join(session)).chain(archived).flat_map(|dir| launches(&trace_in(&dir))).filter(|launch| millis(launch.registered_ms) <= near.saturating_add(NEAR_MS)).max_by_key(|launch| launch.registered_ms)
}

fn placed(agent: &str, session: &str) -> Option<(i64, String)> {
    if agent.is_empty() || agent.contains('/') || agent.starts_with('.') {
        return None;
    }
    let text = fs::read_to_string(fleet_dir().join(".placed").join(agent).join("session.json")).ok()?;
    let record: Value = serde_json::from_str(&text).ok()?;
    (record["name"] == session).then(|| Some((record["created_ms"].as_i64()?, format!("session {session} registered (fleet session record placed on this host)"))))?
}

fn registered(db: &Connection, session: &str, floor: i64, near: i64) -> Option<(i64, String)> {
    let created: String = db.query_row("SELECT created FROM fleet_sessions WHERE name = ?1", [session], |row| row.get(0)).optional().ok()??;
    ms(&created).filter(|at| *at >= floor && *at <= near.saturating_add(NEAR_MS)).map(|at| (at, format!("session {session} registered (threads fleet_sessions)")))
}

fn ticket(records: &Path, session: &str, landed_sha: &str, floor: i64) -> Option<Found> {
    let mut found: Vec<Found> = Vec::new();
    for entry in fs::read_dir(land_queue::queues(records)).into_iter().flatten().flatten() {
        let queue = entry.path();
        for area in ["", "done", "withdrawn", "quarantine"] {
            for file in fs::read_dir(queue.join(area)).into_iter().flatten().flatten() {
                let path = file.path();
                let stamp = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse::<i64>().ok());
                if path.extension().is_none_or(|extension| extension != "json") || stamp.is_none_or(|micros| micros / 1000 < floor) {
                    continue;
                }
                if let Ok(ticket) = queue::read(&path)
                    && ticket.session.as_deref() == Some(session)
                {
                    found.push(Found { path, queue: queue.clone(), ticket });
                }
            }
        }
    }
    let landed = |found: &Found| found.ticket.verdict.as_deref().is_some_and(|verdict| verdict.starts_with("landed"));
    let exact = |found: &Found| !landed_sha.is_empty() && found.ticket.verdict.as_deref() == Some(format!("landed {landed_sha}").as_str());
    found.sort_by_key(|found| (exact(found), landed(found), found.ticket.queued_micros));
    found.pop()
}

fn landing_spans(queue: &Path, stem: &str) -> (Option<i64>, Option<i64>) {
    let id = format!("landing:{stem}");
    let text = fs::read_to_string(Spans::beside(&queue.join("logs")).path()).unwrap_or_default();
    let spans: Vec<Span> = text.lines().filter(|line| line.contains(&id)).filter_map(|line| serde_json::from_str(line).ok()).collect();
    let seconds = |at: f64| (at * 1000.0).round() as i64;
    let root = spans.iter().find(|span| span.id == id).map(|span| seconds(span.end));
    let checking = spans.iter().find(|span| span.parent.as_deref() == Some(id.as_str()) && span.name == "queued").map(|span| seconds(span.end));
    (checking, root)
}

fn receipt(session: &str, after: i64) -> Option<(i64, String)> {
    let db = read_only(&Store::host_path().ok()?)?;
    let at: Option<i64> = db
        .query_row("SELECT min(claimed_ms) FROM receipts WHERE session = ?1 AND (receipt LIKE 'turn:%' OR receipt LIKE 'held:%') AND claimed_ms >= ?2", params![session, after - 5_000], |row| row.get(0))
        .ok()?;
    at.map(|at| (at, "report receipt (run ledger receipts)".to_owned()))
}

fn after_landing(records: &Path, sha: &str) -> Option<(i64, String)> {
    let text = fs::read_to_string(records.join("safe-push/post-land.tsv")).ok()?;
    let rows: Vec<Vec<&str>> = text.lines().map(|line| line.split('\t').collect()).collect();
    let row = |head: &str| rows.iter().find(|fields| fields.get(2) == Some(&head) && fields.get(3).is_some_and(|class| *class != "transient")).cloned();
    let mut found = row(sha)?;
    if let Some(covering) = found.get(7).filter(|covering| !covering.is_empty()).map(|covering| (*covering).to_owned()) {
        found = row(&covering).unwrap_or(found);
    }
    let at: i64 = found.first()?.parse().ok()?;
    Some((at * 1000, format!("after-landing run {} on {} (post-land.tsv)", found.get(3)?, short(found.get(2)?))))
}

fn promoted(records: &Path, item: &str, sha: &str, landed_ms: i64) -> Option<(i64, String)> {
    let repository = item.split('#').next()?;
    let checkout = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code").join(repository).join("main");
    let journal = |path: PathBuf, event: Option<&str>, field: &str, source: &str| -> Vec<(i64, String, String)> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|value| event.is_none_or(|event| value["event"] == event))
            .filter_map(|value| Some((value["at"].as_i64()? * 1000, value[field].as_str()?.to_owned(), source.to_owned())))
            .collect()
    };
    let mut candidates = journal(records.join("north/runtime-pins.jsonl"), Some("promoted"), "sha", "runtime-pins.jsonl, promoted");
    candidates.extend(journal(records_mirror::state_home().join("north/runtime-swaps.jsonl"), None, "runtime", "runtime-swaps.jsonl on this host"));
    candidates.sort();
    candidates.into_iter().filter(|(at, _, _)| *at + 1000 >= landed_ms).find(|(_, runtime, _)| contains(&checkout, sha, runtime)).map(|(at, runtime, source)| (at, format!("runtime {} live ({source})", short(&runtime))))
}

fn contains(checkout: &Path, sha: &str, runtime: &str) -> bool {
    Command::new("git").arg("-C").arg(checkout).args(["merge-base", "--is-ancestor", sha, runtime]).output().is_ok_and(|output| output.status.success())
}
