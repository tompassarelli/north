//! `threads queue`: add queues a job for a placement, take claims the oldest for some placements,
//! take also records that its --by poller polled for those placements, so the list can name why a job still waits;
//! msg records its result on the job's issue, drop retires a queued one; follows boxes wait here
//! for follows-release.
use std::io::Read;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::threads::verb::{Args, Context, Failure, Outcome, event, immediate, integer, is_native_ref, item_ref, open, table, text_or_null};
use crate::foundation::clock::Timestamp;
use crate::platform::pyjson::{self, Value};

const POLL: &str = "queue-poll:";
const STALLED_MINUTES: i64 = 5;

static FOLLOWS_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\W*Follows:[ \t]*`?(.+?)`?[ \t]*$").expect("follows pattern"));

struct Job {
    id: i64,
    placement: String,
    item: Option<String>,
    cmd: String,
    state: String,
    queued: String,
    taken: Option<String>,
    finished: Option<String>,
    result: Option<String>,
}

const JOB: &str = "id, placement, item, cmd, state, queued, taken, finished, result";

fn job(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get(0)?,
        placement: row.get(1)?,
        item: row.get(2)?,
        cmd: row.get(3)?,
        state: row.get(4)?,
        queued: row.get(5)?,
        taken: row.get(6)?,
        finished: row.get(7)?,
        result: row.get(8)?,
    })
}

fn cmd_of(job: &Job) -> Result<Value, Failure> {
    pyjson::loads(job.cmd.as_bytes()).map_err(|_| Failure::Refused(format!("job {} has an unreadable cmd", job.id)))
}

pub(super) fn run(argv: &[String], context: &Context, stdin: &mut dyn Read) -> Outcome {
    let mut at = 0;
    while let Some(arg) = argv.get(at).filter(|arg| arg.starts_with("--")) {
        at += if arg == "--by" { 2 } else { 1 };
    }
    let (head, action) = argv.split_at(at.min(argv.len()));
    let head = Args::parse(head, &[], &["--json"])?;
    let by = head.get("--by").map_or_else(|| context.actor.clone(), str::to_owned);
    let Some((name, rest)) = action.split_first() else { return list(context, head.has("--json")) };
    let mut words = rest.to_vec();
    if let Some(end) = words.iter().position(|word| word == "--").filter(|_| name != "add") {
        words.remove(end);
    }
    let rest = words.as_slice();
    match name.as_str() {
        "follow" => follow(rest, context, stdin),
        "follow-list" => follow_list(context),
        "follow-release" | "follow-started" | "follow-alert" => follow_step(name, rest, context),
        "add" => add(rest, context),
        "take" => take(rest, context, &by),
        "msg" => msg(rest, context, &by),
        "drop" => drop(rest, context, &by),
        other => Err(Failure::Usage(format!("unknown action {other}"))),
    }
}

fn id_of(text: &str) -> Result<i64, Failure> {
    text.parse().map_err(|_| Failure::Usage(format!("{text} is not a job id")))
}

fn follow(argv: &[String], context: &Context, stdin: &mut dyn Read) -> Outcome {
    let args = Args::parse(argv, &["--tier", "--parent", "--repo"], &[])?;
    let ([item], Some(tier), Some(parent)) = (args.positional.as_slice(), args.get("--tier"), args.get("--parent")) else {
        return Err(Failure::Usage("usage: threads queue follow REF --tier T --parent NAME [--repo REPO]".into()));
    };
    let mut brief = String::new();
    stdin.read_to_string(&mut brief).map_err(|error| Failure::Refused(error.to_string()))?;
    let follows = FOLLOWS_LINE.captures(&brief).ok_or_else(|| Failure::Refused("queue follow: the brief on stdin has no Follows: line".into()))?[1].trim().to_owned();
    let (repo, number) = item_ref(item, context)?;
    let item = format!("{repo}#{number}");
    let repo = args.get("--repo").filter(|named| !named.is_empty()).map_or(repo, str::to_owned);
    let cmd = Value::object([("repo", repo.into()), ("tier", tier.into()), ("parent", parent.into()), ("follows", follows.into()), ("brief", brief.into())]);
    insert(context, "follows", Some(&item), &cmd)
}

fn insert(context: &Context, placement: &str, item: Option<&str>, cmd: &Value) -> Outcome {
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    tx.execute("INSERT INTO job_queue(placement, item, cmd, queued) VALUES (?1, ?2, ?3, ?4)", params![placement, item, pyjson::dumps(cmd), context.now.iso()])?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(format!("{id}\n"))
}

fn follow_list(context: &Context) -> Outcome {
    let db = open(context.database, false)?;
    let mut statement = db.prepare(&format!("SELECT {JOB} FROM job_queue WHERE placement = 'follows' AND state IN ('queued','taken') ORDER BY id"))?;
    let jobs = statement.query_map([], job)?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut boxes = Vec::new();
    for job in &jobs {
        let mut entry = Value::object([
            ("id", integer(job.id)),
            ("item", text_or_null(job.item.clone())),
            ("state", job.state.as_str().into()),
            ("queued", job.queued.as_str().into()),
            ("taken", text_or_null(job.taken.clone())),
            ("alerted", text_or_null(job.result.clone())),
        ]);
        if let (Value::Object(fields), Value::Object(ref cmd)) = (&mut entry, cmd_of(job)?) {
            for (key, value) in cmd.iter() {
                match fields.iter_mut().find(|(name, _)| name == key) {
                    Some(field) => field.1 = value.clone(),
                    None => fields.push((key.clone(), value.clone())),
                }
            }
        }
        boxes.push(entry);
    }
    Ok(format!("{}\n", pyjson::dumps(&Value::Array(boxes))))
}

fn follow_step(step: &str, argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &[], &[])?;
    let alert = step == "follow-alert";
    let (Some(id), true) = (args.positional.first(), if alert { args.positional.len() > 1 } else { args.positional.len() == 1 }) else {
        return Err(Failure::Usage(format!("usage: threads queue {step} ID{}", if alert { " TEXT..." } else { "" })));
    };
    let id = id_of(id)?;
    let (state, change) = match step {
        "follow-release" => ("queued", "state = 'taken', taken = ?1"),
        "follow-started" => ("taken", "state = 'done', finished = ?1"),
        _ => ("taken", "result = ?1"),
    };
    let value = if alert { args.positional[1..].join(" ") } else { context.now.iso() };
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let unalerted = if alert { " AND result IS NULL" } else { "" };
    let changed = tx.execute(&format!("UPDATE job_queue SET {change} WHERE id = ?2 AND placement = 'follows' AND state = ?3{unalerted}"), params![value, id, state])?;
    tx.commit()?;
    if changed != 1 {
        return Err(Failure::Refused(format!("queue {step}: box {id} is not an open follows box in state {state}")));
    }
    Ok(String::new())
}

fn add(argv: &[String], context: &Context) -> Outcome {
    let (Some(placement), Some(item)) = (argv.first(), argv.get(1)) else {
        return Err(Failure::Usage("usage: threads queue add PLACEMENT REF|- -- CMD...".into()));
    };
    let cmd = argv[2..].strip_prefix(&["--".to_owned()]).unwrap_or(&argv[2..]);
    if cmd.is_empty() {
        return Err(Failure::Refused("queue add: no command after --".into()));
    }
    let item = if item == "-" { None } else { Some(item_ref(item, context).map(|(repo, number)| format!("{repo}#{number}"))?) };
    insert(context, placement, item.as_deref(), &Value::Array(cmd.iter().map(|word| word.as_str().into()).collect()))
}

fn take(argv: &[String], context: &Context, by: &str) -> Outcome {
    let [named] = argv else { return Err(Failure::Usage("usage: threads queue [--by POLLER] take PLACEMENT[,PLACEMENT...]".into())) };
    let placements: Vec<&str> = named.split(',').collect();
    let marks = vec!["?"; placements.len()].join(",");
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let name = format!("{POLL}{named}");
    tx.execute("DELETE FROM job_runs WHERE name = ?1 AND host = ?2", params![name, by])?;
    tx.execute("INSERT INTO job_runs(name, host, started, finished, rc) VALUES (?1, ?2, ?3, ?3, 0)", params![name, by, context.now.iso()])?;
    let found = tx
        .query_row(&format!("SELECT {JOB} FROM job_queue WHERE placement IN ({marks}) AND state IN ('queued','requeued') ORDER BY id LIMIT 1"), params_from_iter(&placements), job)
        .optional()?;
    let Some(found) = found else {
        tx.commit()?;
        return Ok(String::new());
    };
    tx.execute("UPDATE job_queue SET state = 'taken', taken = ?1 WHERE id = ?2", params![context.now.iso(), found.id])?;
    tx.commit()?;
    let taken = Value::object([("id", integer(found.id)), ("placement", found.placement.as_str().into()), ("item", text_or_null(found.item.clone())), ("cmd", cmd_of(&found)?)]);
    Ok(format!("{}\n", pyjson::dumps(&taken)))
}

fn msg(argv: &[String], context: &Context, by: &str) -> Outcome {
    let (Some(id), Some(status), true) = (argv.first(), argv.get(1), argv.len() > 2) else {
        return Err(Failure::Usage("usage: threads queue msg ID done|failed|requeued TEXT...".into()));
    };
    if !["done", "failed", "requeued"].contains(&status.as_str()) {
        return Err(Failure::Usage(format!("{status} is not one of done, failed, requeued")));
    }
    let id = id_of(id)?;
    let text = argv[2..].join(" ");
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let found = tx.query_row(&format!("SELECT {JOB} FROM job_queue WHERE id = ?1"), [id], job).optional()?;
    let Some(found) = found.filter(|found| found.state == "taken") else { return Err(Failure::Refused(format!("queue msg: job {id} is not taken"))) };
    if status == "requeued" {
        tx.execute("UPDATE job_queue SET state = 'requeued', taken = NULL, result = ?1 WHERE id = ?2", params![text, id])?;
    } else {
        tx.execute("UPDATE job_queue SET state = ?1, finished = ?2, result = ?3 WHERE id = ?4", params![status, context.now.iso(), text, id])?;
    }
    let item = found.item.as_deref();
    if let Some(item) = item.filter(|item| is_native_ref(item)) {
        event(&tx, Some(item), "note", by, Value::object([("text", format!("job {id} {status}: {text}").into())]), context.now)?;
    }
    tx.commit()?;
    if let Some(item) = item.filter(|item| !is_native_ref(item)) {
        context.outside.scan(&text)?;
        let (repo, number) = item_ref(item, context)?;
        let (number, full) = (number.to_string(), format!("{}/{repo}", crate::threads::catalog::CATALOG.owner));
        context.gh(&["issue", "comment", &number, "-R", &full, "--body", &format!("Job {id} {status}:\n\n{text}")])?;
    }
    Ok(format!("job {id} {status}\n"))
}

fn drop(argv: &[String], context: &Context, by: &str) -> Outcome {
    let (Some(id), true) = (argv.first(), argv.len() > 1) else { return Err(Failure::Usage("usage: threads queue drop ID REASON...".into())) };
    let id = id_of(id)?;
    let reason = argv[1..].join(" ");
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    let state: Option<String> = tx.query_row("SELECT state FROM job_queue WHERE id = ?1", [id], |row| row.get(0)).optional()?;
    if state.is_none() || !drop_job(&tx, id, &reason, by, context.now)? {
        return Err(Failure::Refused(format!("queue drop: job {id} is {}, not queued", state.as_deref().unwrap_or("unknown"))));
    }
    tx.commit()?;
    Ok(format!("job {id} dropped\n"))
}

fn drop_job(db: &Connection, id: i64, reason: &str, by: &str, now: Timestamp) -> rusqlite::Result<bool> {
    let changed = db.execute("UPDATE job_queue SET state = 'dropped', finished = ?1, result = ?2 WHERE id = ?3 AND state IN ('queued','requeued')", params![now.iso(), reason, id])?;
    if changed == 1 {
        let item: Option<String> = db.query_row("SELECT item FROM job_queue WHERE id = ?1", [id], |row| row.get(0))?;
        event(db, item.as_deref(), "note", by, Value::object([("job", integer(id)), ("text", format!("job {id} dropped: {reason}").into())]), now)?;
    }
    Ok(changed == 1)
}

fn list(context: &Context, json: bool) -> Outcome {
    let db = open(context.database, false)?;
    let latest = |what: &str| format!("(SELECT {what} FROM job_runs j WHERE j.name = 'queue-' || q.id ORDER BY started DESC LIMIT 1)");
    let sql = format!("SELECT {JOB}, {}, {} FROM job_queue q ORDER BY id", latest("host"), latest("host || ' ' || started || ' rc=' || ifnull(rc, '-')"));
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map([], |row| Ok((job(row)?, row.get::<_, Option<String>>(9)?, row.get::<_, Option<String>>(10)?)))?;
    let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let polls = polls(&db)?;
    if json {
        let jobs = rows.into_iter().map(|(job, host, _)| {
            let waiting = waiting(&job, &polls, context.now);
            Value::object([
                ("id", integer(job.id)),
                ("placement", job.placement.into()),
                ("state", job.state.into()),
                ("queued", job.queued.into()),
                ("taken", text_or_null(job.taken)),
                ("finished", text_or_null(job.finished)),
                ("host", text_or_null(host)),
                ("waiting", text_or_null(waiting)),
            ])
        });
        return Ok(format!("{}\n", pyjson::dumps(&Value::Array(jobs.collect()))));
    }
    let mut out = vec![["id", "placement", "item", "state", "queued", "wait", "finished", "job_run", "cmd"].map(str::to_owned).to_vec()];
    for (job, _, run) in &rows {
        let item = match &job.item {
            Some(item) if context.no_native.is_some() && is_native_ref(item) => "native".to_owned(),
            Some(item) => item.clone(),
            None => "-".to_owned(),
        };
        let line = [job.id.to_string(), job.placement.clone(), item, job.state.clone(), job.queued.clone(), wait(job, context.now)];
        let tail = [job.finished.clone().unwrap_or_else(|| "-".into()), run.clone().unwrap_or_else(|| "-".into()), follows_text(&cmd_of(job)?)];
        out.push([line.to_vec(), tail.to_vec()].concat());
    }
    let reasons: String = rows.iter().filter_map(|(job, _, _)| waiting(job, &polls, context.now)).map(|reason| format!("{reason}\n")).collect();
    Ok(table(&out) + &reasons)
}

struct Poll {
    host: String,
    placements: String,
    at: String,
}

fn polls(db: &Connection) -> rusqlite::Result<Vec<Poll>> {
    let mut statement = db.prepare("SELECT name, host, max(started) FROM job_runs WHERE name GLOB ?1 GROUP BY name, host")?;
    let rows = statement.query_map([format!("{POLL}*")], |row| {
        let name: String = row.get(0)?;
        Ok(Poll { placements: name.strip_prefix(POLL).unwrap_or(&name).to_owned(), host: row.get(1)?, at: row.get(2)? })
    })?;
    rows.collect()
}

fn waiting(job: &Job, polls: &[Poll], now: Timestamp) -> Option<String> {
    let minutes = (now.unix_seconds() - Timestamp::parse(&job.queued).ok()?.unix_seconds()).div_euclid(60);
    if !matches!(job.state.as_str(), "queued" | "requeued") || minutes < STALLED_MINUTES {
        return None;
    }
    let last = polls.iter().filter(|poll| poll.placements.split(',').any(|placement| placement == job.placement)).max_by(|a, b| a.at.cmp(&b.at));
    Some(match last {
        Some(poll) => format!("job {} waits {minutes}m: {} last polled this ledger for {} at {}", job.id, poll.host, poll.placements, poll.at),
        None => format!("job {} waits {minutes}m: no runner has polled this ledger for {}", job.id, job.placement),
    })
}

/// Minutes from queued to taken, or to dropped, or to now while it waits.
fn wait(job: &Job, now: Timestamp) -> String {
    let end = job.taken.as_deref().or(job.finished.as_deref().filter(|_| job.state == "dropped"));
    let at = |text: &str| Timestamp::parse(text).map(Timestamp::unix_seconds).unwrap_or(0);
    let end = end.map_or_else(|| now.unix_seconds(), at);
    format!("{}m", (end - at(&job.queued)).div_euclid(60))
}

fn follows_text(cmd: &Value) -> String {
    if cmd.is_object() {
        return format!("follows {} on {}", cmd["follows"].as_str().unwrap_or(""), cmd["tier"].as_str().unwrap_or(""));
    }
    let words = cmd.as_array().unwrap_or(&[]);
    words.iter().map(|word| crate::threads::client::shell_quote(&word.as_string().map_or_else(String::new, |text| text.lossy().into_owned()))).collect::<Vec<_>>().join(" ")
}
