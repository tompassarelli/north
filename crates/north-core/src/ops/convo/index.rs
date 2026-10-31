use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::IsTerminal;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use rustix::fs::{FlockOperation, flock};

use super::db::{Checkpoint, Pending, connect, forget, save, store};
use super::discover::{Found, Stat, discover, transcript_roots};
use super::env::Env;
use super::failure::Result;
use super::json::Json;
use super::lines::{CHUNK, HEAD_BYTES, LINE_CAP, Source, head_hash, scan_lines};
use super::records::{Kind, Meta, extract_claude, extract_codex, wanted};

const BATCH: usize = 5000;

pub(super) struct Indexed {
    pub files: usize,
    pub messages: usize,
    pub bytes: u64,
}

struct Row {
    id: i64,
    dev: i64,
    ino: i64,
    size: i64,
    dsize: i64,
    zst: bool,
    mtime: Option<f64>,
    head: Option<String>,
    off: i64,
    nline: i64,
}

struct Item {
    found: Found,
    row: Option<Row>,
    reset: bool,
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

fn file_row(con: &Connection, path: &str) -> Result<Option<Row>> {
    Ok(con
        .query_row("SELECT id, dev, ino, size, dsize, zst, mtime, head, off, nline FROM files WHERE path = ?", [path], |row| {
            Ok(Row {
                id: row.get(0)?,
                dev: row.get(1)?,
                ino: row.get(2)?,
                size: row.get(3)?,
                dsize: row.get(4)?,
                zst: row.get::<_, i64>(5)? != 0,
                mtime: row.get(6)?,
                head: row.get(7)?,
                off: row.get(8)?,
                nline: row.get(9)?,
            })
        })
        .optional()?)
}

fn same_inode(path: &str, stat: &Stat) -> bool {
    fs::metadata(path).is_ok_and(|meta| Stat::of(&meta).dev == stat.dev && Stat::of(&meta).ino == stat.ino)
}

type Known = (Option<i64>, Option<i64>, i64, i64, Option<f64>);

fn known_files(con: &Connection) -> Result<HashMap<String, Known>> {
    let mut statement = con.prepare("SELECT path, dev, ino, dsize, zst, mtime FROM files")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn unchanged(known: &Known, stat: &Stat, zst: bool) -> bool {
    known.0 == Some(stat.dev as i64)
        && known.1 == Some(stat.ino as i64)
        && known.2 == stat.size as i64
        && known.3 == i64::from(zst)
        && (stat.mtime_secs() - known.4.unwrap_or(0.0)).abs() < 1e-3
}

fn row_matches(row: &Row, stat: &Stat, zst: bool) -> bool {
    row.zst == zst
        && row.ino == stat.ino as i64
        && row.dev == stat.dev as i64
        && stat.size as i64 == row.dsize
        && (stat.mtime_secs() - row.mtime.unwrap_or(0.0)).abs() < 1e-3
}

/// A fully read file that only changed between plain and archived form keeps its rows.
fn adopt(con: &Connection, row: &Row, stat: &Stat, zst: bool) -> Result<bool> {
    let same = (stat.mtime_secs() - row.mtime.unwrap_or(0.0)).abs() < 1e-3;
    let complete = row.size > 0 && row.off == row.size;
    if zst == row.zst || !complete || !same || !(zst || stat.size as i64 == row.size) {
        return Ok(false);
    }
    con.execute(
        "UPDATE files SET zst=?, dev=?, ino=?, dsize=?, mtime=? WHERE id=?",
        rusqlite::params![i64::from(zst), stat.dev as i64, stat.ino as i64, stat.size as i64, stat.mtime_secs(), row.id],
    )?;
    Ok(true)
}

fn needs_reset(row: &Row, stat: &Stat, zst: bool, logical: &str) -> bool {
    let head = if zst { None } else { head_hash(logical, (row.size.max(0) as usize).min(HEAD_BYTES)) };
    zst
        || row.ino != stat.ino as i64
        || row.dev != stat.dev as i64
        || (stat.size as i64) < row.off
        || stat.size as i64 == row.dsize
        || head.is_none()
        || head != row.head
}

/// The file's row, found by path or by inode when the file moved; `None` means a new file.
fn known_row(con: &Connection, item: &Found, known: &HashMap<String, Known>, by_inode: &HashMap<(i64, i64), &String>) -> Result<Option<Option<Row>>> {
    let zst = item.disk != item.logical;
    let alias = match known.get(&item.logical) {
        None => match by_inode.get(&(item.stat.dev as i64, item.stat.ino as i64)) {
            Some(path) if same_inode(path, &item.stat) => (*path).clone(),
            _ => return Ok(Some(None)),
        },
        Some(k) if unchanged(k, &item.stat, zst) => return Ok(None),
        Some(_) => item.logical.clone(),
    };
    let Some(row) = file_row(con, &alias)? else { return Ok(None) };
    if alias != item.logical {
        con.execute("UPDATE files SET path = ? WHERE id = ?", rusqlite::params![item.logical, row.id])?;
    }
    Ok(Some(Some(row)))
}

/// Returns the files to read; archives of fully read files are adopted in place.
fn plan_refresh(con: &Connection, found: Vec<Found>) -> Result<Vec<Item>> {
    let known = known_files(con)?;
    let by_inode: HashMap<(i64, i64), &String> = known.iter().filter_map(|(path, k)| Some(((k.0?, k.1?), path))).collect();
    let mut todo = Vec::new();
    for item in found {
        let zst = item.disk != item.logical;
        match known_row(con, &item, &known, &by_inode)? {
            None => {}
            Some(None) => todo.push(Item { found: item, row: None, reset: false }),
            Some(Some(row)) => {
                if row_matches(&row, &item.stat, zst) || adopt(con, &row, &item.stat, zst)? {
                    continue;
                }
                let reset = needs_reset(&row, &item.stat, zst, &item.logical);
                todo.push(Item { found: item, row: Some(row), reset });
            }
        }
    }
    Ok(todo)
}

struct Progress {
    files: usize,
    total: u64,
    done_files: usize,
    bytes: u64,
    messages: usize,
    started: Instant,
    last: Option<Instant>,
    on: bool,
}

impl Progress {
    fn tick(&mut self) {
        if !self.on || self.last.is_some_and(|last| last.elapsed() < Duration::from_secs(1)) {
            return;
        }
        self.last = Some(Instant::now());
        let rate = self.bytes as f64 / self.started.elapsed().as_secs_f64().max(1e-9);
        let eta = if rate > 0.0 { (self.total.saturating_sub(self.bytes)) as f64 / rate } else { 0.0 };
        let gib = |bytes: u64| bytes as f64 / (1u64 << 30) as f64;
        eprint!(
            "\r\x1b[Kindexing {}/{} files  {:.1}/{:.1} GiB  {} msgs  eta {}m{:02}s",
            self.done_files, self.files, gib(self.bytes), gib(self.total), self.messages, eta as u64 / 60, eta as u64 % 60
        );
    }
}

struct Reading<'a> {
    con: &'a Connection,
    env: &'a Env,
    next_id: i64,
    progress: &'a mut Progress,
}

fn register(con: &Connection, found: &Found) -> Result<i64> {
    let path = Path::new(&found.logical);
    let agent = path
        .parent()
        .and_then(Path::file_name)
        .filter(|name| *name == "subagents")
        .and_then(|_| path.file_name())
        .map(|name| name.to_string_lossy().trim_end_matches(".jsonl").trim_start_matches("agent-").to_owned());
    con.execute(
        "INSERT INTO files(path, dev, ino, provider, account, project, agent) VALUES(?,?,?,?,?,?,?)",
        rusqlite::params![found.logical, found.stat.dev as i64, found.stat.ino as i64, found.provider, found.account, found.hint, agent],
    )?;
    Ok(con.last_insert_rowid())
}

fn read_file(at: &mut Reading, item: &Item) -> Result<usize> {
    let found = &item.found;
    let zst = found.disk != found.logical;
    if let (Some(row), true) = (&item.row, item.reset) {
        forget(at.con, row.id)?;
    }
    let (file, start, mut line_no) = match &item.row {
        None => (register(at.con, found)?, 0u64, 0i64),
        Some(row) if item.reset => (row.id, 0, 0),
        Some(row) => (row.id, row.off as u64, row.nline),
    };
    let Ok(mut source) = Source::open(&at.env.zstd, &found.disk, start) else { return Ok(0) };
    let mut state = Scan { meta: Meta::default(), pending: Vec::new(), added: 0, first_ts: None, last_ts: None, off: start };
    let outcome = scan_lines(&mut source, start, CHUNK, LINE_CAP, |from, end, line| {
        line_no += 1;
        state.off = end;
        at.progress.bytes += end - from;
        let Some(line) = line.filter(|line| wanted(found.kind, line)) else { return Ok(()) };
        state.take(found.kind, line, line_no);
        if state.pending.len() >= BATCH {
            state.flush(at, file, &found.hint, line_no).map_err(|failure| std::io::Error::other(failure.to_string()))?;
            at.progress.tick();
        }
        Ok(())
    });
    let intact = source.head();
    let closed = source.close();
    outcome?;
    if !closed {
        forget(at.con, file)?;
        at.con.execute("UPDATE files SET size=0, dsize=0 WHERE id=?", [file])?;
        eprintln!("convo: unreadable archive, skipped: {}", found.disk);
        return Ok(0);
    }
    state.flush(at, file, &found.hint, line_no)?;
    let head = if zst { Some(intact) } else { head_hash(&found.logical, (found.stat.size as usize).min(HEAD_BYTES)) };
    at.con.execute(
        "UPDATE files SET size=?, dsize=?, zst=?, mtime=?, head=?, dev=?, ino=? WHERE id=?",
        rusqlite::params![
            if zst { state.off } else { found.stat.size } as i64,
            found.stat.size as i64,
            i64::from(zst),
            found.stat.mtime_secs(),
            head,
            found.stat.dev as i64,
            found.stat.ino as i64,
            file
        ],
    )?;
    Ok(state.added)
}

struct Scan {
    meta: Meta,
    pending: Vec<Pending>,
    added: usize,
    first_ts: Option<String>,
    last_ts: Option<String>,
    off: u64,
}

impl Scan {
    fn take(&mut self, kind: Kind, line: &[u8], line_no: i64) {
        let Some(record) = Json::parse(line).filter(Json::is_object) else { return };
        let (messages, meta) = if kind == Kind::Claude { extract_claude(&record) } else { extract_codex(&record) };
        self.meta.merge(meta);
        let ts = record.text_of("timestamp").map(str::to_owned);
        if let Some(ts) = &ts {
            self.first_ts.get_or_insert_with(|| ts.clone());
            self.last_ts = Some(ts.clone());
        }
        self.pending.extend(messages.into_iter().map(|m| Pending { line: line_no, ts: ts.clone(), role: m.role, tool: m.tool, text: m.text }));
    }

    fn flush(&mut self, at: &mut Reading, file: i64, hint: &str, line_no: i64) -> Result<()> {
        at.next_id = store(at.con, file, &self.pending, at.next_id)?;
        let count = self.pending.len();
        self.added += count;
        at.progress.messages += count;
        save(
            at.con,
            &Checkpoint { file, off: self.off, line: line_no, meta: &self.meta, hint, first_ts: &self.first_ts, last_ts: &self.last_ts, count },
        )?;
        self.pending.clear();
        at.con.execute_batch("COMMIT; BEGIN")?;
        Ok(())
    }
}

fn first_free_id(con: &Connection) -> Result<i64> {
    let max = |sql: &str| con.query_row(sql, [], |row| row.get::<_, i64>(0));
    let meta = con.query_row("SELECT COALESCE(MAX(v), 0) FROM meta WHERE k = 'next_id'", [], |row| row.get::<_, f64>(0)).unwrap_or(0.0) as i64;
    Ok((1 + max("SELECT COALESCE(MAX(id), 0) FROM msg")?).max(1 + max("SELECT COALESCE(MAX(id), 0) FROM body")?).max(meta))
}

/// Incremental pass over every configured root.
pub(super) fn index(con: &Connection, env: &Env, quiet: bool, verbose: bool) -> Result<Indexed> {
    let roots = transcript_roots(env)?;
    let todo = plan_refresh(con, discover(&roots)?)?;
    let total: u64 = todo
        .iter()
        .map(|item| item.found.stat.size.saturating_sub(item.row.as_ref().filter(|_| !item.reset).map_or(0, |row| row.off as u64)))
        .sum();
    if total > 64 << 20 {
        let _ = rustix::process::nice(10);
    }
    let mut progress =
        Progress { files: todo.len(), total, done_files: 0, bytes: 0, messages: 0, started: Instant::now(), last: None, on: !quiet && std::io::stderr().is_terminal() };
    con.execute_batch("BEGIN")?;
    let mut reading = Reading { con, env, next_id: first_free_id(con)?, progress: &mut progress };
    let (mut added, mut uncommitted) = (0, 0);
    for item in &todo {
        let count = read_file(&mut reading, item)?;
        added += count;
        uncommitted += count;
        if uncommitted >= BATCH {
            con.execute_batch("COMMIT; BEGIN")?;
            uncommitted = 0;
        }
        reading.progress.done_files += 1;
        if verbose && count > 0 {
            eprintln!("\r\x1b[K{count:6} msgs  {}", item.found.logical);
        }
        reading.progress.tick();
    }
    let names: Vec<&str> = roots.iter().map(|root| root.path.as_str()).collect();
    con.execute(
        "INSERT OR REPLACE INTO meta(k, v) VALUES('last_index', ?), ('last_roots', ?)",
        rusqlite::params![now().to_string(), serde_json::to_string(&names).unwrap_or_default()],
    )?;
    con.execute_batch("COMMIT")?;
    if progress.on {
        eprint!("\r\x1b[K");
    }
    Ok(Indexed { files: todo.len(), messages: added, bytes: progress.bytes })
}

/// Runs an incremental pass under the index lock; `None` if the lock stayed busy.
pub(super) fn refresh(env: &Env, quiet: bool, verbose: bool, wait: Duration, rebuild: bool) -> Result<Option<Indexed>> {
    env.ensure_state()?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(env.lock())?;
    let deadline = Instant::now() + wait;
    while flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        if Instant::now() >= deadline {
            return Ok(None);
        }
        sleep(Duration::from_millis(20));
    }
    if rebuild {
        let db = env.db();
        for suffix in ["", "-wal", "-shm"] {
            let mut name = db.clone().into_os_string();
            name.push(suffix);
            let _ = fs::remove_file(name);
        }
    }
    let con = connect(&env.db(), &env.state, false)?;
    let done = index(&con, env, quiet, verbose);
    drop(lock);
    done.map(Some)
}
