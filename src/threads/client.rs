use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::threads::{board, calibration, claims, ingest_lock, items, jobs, outside, runs, snapshot, staffing, verb};
use crate::foundation::clock::Timestamp;

const READS: [&str; 12] = [
    "job-runs", "show", "ready", "list", "unowned", "run-ids", "summary", "recommend", "calibrate",
    "replay-staffing", "planner-band", "routing-compare",
];
const UNREACHABLE: i32 = 255;
const TIMED_OUT: i32 = 124;
const SSH_TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_WAIT: Duration = Duration::from_secs(10);
const INGEST_WAIT: Duration = Duration::from_secs(5);

pub(crate) struct Client {
    home: PathBuf,
    remote: PathBuf,
    spool: PathBuf,
    snapshot: PathBuf,
    env: BTreeMap<String, String>,
    ssh: Vec<OsString>,
    threads: Vec<OsString>,
    record_wait: Duration,
    ingest_wait: Duration,
}

#[derive(PartialEq)]
enum Flush {
    Clear,
    Busy,
    Down,
}

enum Input {
    Inherit,
    Null,
    Bytes(Vec<u8>),
    File(File),
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let Some(client) = ambient_client() else {
        eprintln!("NorthThreads: no home directory");
        return 1;
    };
    match client.main(arguments, &mut io::stdin(), &mut io::stderr()) {
        Ok(code) => (code & 0xff) as u8,
        Err(error) => {
            eprintln!("NorthThreads: {error}");
            1
        }
    }
}

pub(crate) fn refresh_mirrored_snapshot() {
    let Some(client) = ambient_client() else { return };
    if let Ok(destination) = fs::read_to_string(&client.remote) {
        let _ = client.refresh_snapshot(destination.trim(), true);
    }
}

pub(crate) fn ingest(batch: &[u8]) -> Result<(), String> {
    let client = ambient_client().ok_or("no home directory")?;
    let mut err = Vec::new();
    match client.main(&["ingest".to_owned()], &mut io::Cursor::new(batch), &mut err) {
        Ok(0) => Ok(()),
        Ok(code) => Err(format!("ingest exited {code}: {}", String::from_utf8_lossy(&err).trim())),
        Err(error) => Err(error.to_string()),
    }
}

fn ambient_client() -> Option<Client> {
    let env: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let home = env.get("HOME").cloned()?;
    Some(Client::new(Path::new(&home), env, vec!["ssh".into()], vec![local_threads()]))
}

fn local_threads() -> OsString {
    crate::platform::harness::harness_root().map_or_else(|_| "threads".into(), |root| root.join("bin/threads").into_os_string())
}

impl Client {
    fn new(home: &Path, env: BTreeMap<String, String>, ssh: Vec<OsString>, threads: Vec<OsString>) -> Self {
        Self {
            home: home.to_path_buf(),
            remote: home.join(".config/threads/remote"),
            spool: home.join(".local/state/threads/spool"),
            snapshot: home.join(".local/state/threads/snapshot.db"),
            env,
            ssh,
            threads,
            record_wait: RECORD_WAIT,
            ingest_wait: INGEST_WAIT,
        }
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.env.get(name).map(String::as_str).filter(|value| !value.is_empty())
    }

    fn main(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        let first = argv.first().map(String::as_str);
        if first == Some("queue") && argv.get(1).map(String::as_str) == Some("follow") {
            let mut brief = Vec::new();
            stdin.read_to_end(&mut brief)?;
            let vetted = if self.var(verb::SCHEMA_READY).is_some() { Ok(()) } else { self.vet_follows(argv, &brief) };
            if let Err(why) = vetted {
                writeln!(err, "NorthThreads: queue follow: {why}")?;
                return Ok(1);
            }
            return self.route(argv, &mut io::Cursor::new(brief.clone()), err, Some(&brief));
        }
        self.route(argv, stdin, err, None)
    }

    fn vet_follows(&self, argv: &[String], brief: &[u8]) -> Result<(), String> {
        let Some(follows) = crate::platform::follows::follows_line(&String::from_utf8_lossy(brief)) else { return Ok(()) };
        let repo = argv.iter().skip_while(|arg| *arg != "--repo").nth(1).map(String::as_str).or_else(|| argv.get(2).and_then(|item| item.split_once('#')).map(|(repo, _)| repo));
        let root = self.var("FOLLOWS_RELEASE_ROOT").map_or_else(|| self.home.join("code"), PathBuf::from);
        crate::platform::follows::vet(&follows, repo.map(|repo| root.join(repo).join("main")).as_deref())
    }

    fn route(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write, replay: Option<&[u8]>) -> NorthResult<i32> {
        let first = argv.first().map(String::as_str);
        if first == Some("snapshot-pages") || first == Some("snapshot-pull") {
            return self.snapshot_verb(argv, stdin, err);
        }
        let database = self.var("THREADS_DB").is_some();
        if first == Some("routing-compare") && !database {
            return self.local(argv, Some(&self.snapshot), replay);
        }
        if !database && self.remote.exists() {
            let dest = fs::read_to_string(&self.remote)?.trim().to_owned();
            return match self.proxy(&dest, argv, stdin, err)? {
                Some(code) => Ok(code),
                None => self.local(argv, Some(&self.snapshot), replay),
            };
        }
        if first == Some("ingest") && self.var(verb::SCHEMA_READY).is_none() {
            return self.ingest_locally(argv, stdin, err);
        }
        self.local(argv, None, replay)
    }

    fn ingest_locally(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        let mut data = Vec::new();
        stdin.read_to_end(&mut data)?;
        let ledger = self.var("THREADS_DB").map_or_else(|| self.home.join(".local/state/threads/threads.db"), PathBuf::from);
        let Some(_lock) = ingest_lock::acquire(&ledger.with_file_name("ingest.lock"), self.ingest_wait)? else {
            writeln!(err, "NorthThreads: another ingest holds the ledger; retry")?;
            return Ok(ingest_lock::RETRY);
        };
        self.local(argv, Some(&ledger), Some(&data))
    }

    fn local(&self, argv: &[String], database: Option<&Path>, replay: Option<&[u8]>) -> NorthResult<i32> {
        let verb = argv.first().map_or("", String::as_str);
        let ledger = database.map(Path::to_path_buf).or_else(|| self.var("THREADS_DB").map(PathBuf::from));
        let ledger = ledger.unwrap_or_else(|| self.home.join(".local/state/threads/threads.db"));
        let actor = self.var("THREADS_ACTOR").or_else(|| self.var("USER")).unwrap_or("unknown").to_owned();
        if claims::verbs::VERBS.contains(&verb) {
            let context = claims::verbs::Context { database: &ledger, actor, no_native: self.no_native_reason(), issues: &claims::issues::GitHub, now: Timestamp::now().unix_seconds() };
            return Ok(claims::verbs::run(argv, &context, &mut io::stdout().lock(), &mut io::stderr()));
        }
        let ported: Option<verb::Verb> = if jobs::VERBS.contains(&verb) {
            Some(jobs::run)
        } else if items::VERBS.contains(&verb) {
            Some(items::run)
        } else if board::VERBS.contains(&verb) {
            Some(board::run)
        } else if staffing::VERBS.contains(&verb) {
            Some(staffing::run)
        } else if runs::VERBS.contains(&verb) {
            Some(runs::run)
        } else if calibration::VERBS.contains(&verb) {
            Some(calibration::run)
        } else {
            None
        };
        if let Some(run) = ported.filter(|_| self.var(verb::SCHEMA_READY).is_some()) {
            let live = outside::Live::new(&self.env);
            let replica = ledger == self.snapshot;
            let context = verb::Context { database: &ledger, actor, no_native: self.no_native_reason(), outside: &live, now: Timestamp::now(), replica };
            let mut stdin: Box<dyn Read> = replay.map_or_else(|| Box::new(io::stdin()) as Box<dyn Read>, |brief| Box::new(io::Cursor::new(brief.to_vec())));
            return Ok(run(argv, &context, &mut stdin, &mut io::stdout().lock(), &mut io::stderr()));
        }
        let mut command = Command::new(&self.threads[0]);
        command.args(&self.threads[1..]).args(argv);
        if let Some(database) = database {
            command.env("THREADS_DB", database);
        }
        if let Some(north) = self.var("NORTH_BIN") {
            command.env("NORTH_BIN", north);
        } else if let Ok(own) = std::env::current_exe() {
            command.env("NORTH_BIN", own);
        }
        let Some(input) = replay else { return Ok(code_of(command.status()?)) };
        let mut child = command.stdin(Stdio::piped()).spawn()?;
        child.stdin.take().ok_or_else(|| io::Error::other("no stdin pipe"))?.write_all(input)?;
        Ok(code_of(child.wait()?))
    }

    fn proxy(&self, dest: &str, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<Option<i32>> {
        let first = argv.first().map(String::as_str);
        let has = |flag: &str| argv.iter().any(|arg| arg == flag);
        if first == Some("planner-band") || first == Some("recommend") && has("--planner") {
            let code = self.ssh(dest, argv, Input::Inherit, false, err)?;
            if code != UNREACHABLE {
                return Ok(Some(code));
            }
            if self.snapshot.exists() {
                writeln!(err, "{}", self.stale_line(dest)?)?;
                return Ok(None);
            }
            return Ok(Some(code));
        }
        let mut argv = argv.to_vec();
        if first == Some("agent-event") {
            if let Err(message) = agent_event_checks(&argv) {
                writeln!(err, "NorthThreads: {message}")?;
                return Ok(Some(1));
            }
            if !argv.iter().any(|arg| arg == "--key" || arg.starts_with("--key=")) {
                let key = fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().to_owned();
                argv.extend(["--key".to_owned(), key]);
            }
        }
        if first == Some("job-run") {
            let code = self.ssh(dest, &argv, Input::Null, false, err)?;
            if code != UNREACHABLE && code != TIMED_OUT {
                return Ok(Some(code));
            }
            self.spool_argv(&argv)?;
            return Ok(Some(0));
        }
        let flush = self.flush_spool(dest, err)?;
        let reachable = flush != Flush::Down;
        let clear = flush == Flush::Clear;
        if first == Some("ingest") {
            let mut data = Vec::new();
            stdin.read_to_end(&mut data)?;
            let mut state = "unreachable";
            if clear {
                let code = self.ssh(dest, &argv, Input::Bytes(data.clone()), false, err)?;
                if code == ingest_lock::RETRY {
                    state = "busy";
                } else if code != UNREACHABLE {
                    if code == 0 && has_usage(&data)? {
                        self.refresh_snapshot(dest, true)?;
                    }
                    return Ok(Some(code));
                }
            }
            fs::create_dir_all(&self.spool)?;
            let path = self.spool.join(format!("{}.jsonl", spool_stem()));
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, &data)?;
            fs::rename(&tmp, &path)?;
            writeln!(err, "NorthThreads: {dest} {state}; spooled ingest to {}", path.display())?;
            return Ok(Some(0));
        }
        if first == Some("agent-event") {
            let code = if clear { self.ssh(dest, &argv, Input::Null, false, err)? } else { UNREACHABLE };
            if code != UNREACHABLE {
                return Ok(Some(code));
            }
            self.spool_argv(&argv)?;
            return Ok(Some(0));
        }
        let follow = first == Some("queue") && argv.get(1).map(String::as_str) == Some("follow");
        let input = if follow {
            let mut brief = Vec::new();
            stdin.read_to_end(&mut brief)?;
            Input::Bytes(brief)
        } else {
            Input::Inherit
        };
        let code = if reachable { self.ssh(dest, &argv, input, false, err)? } else { UNREACHABLE };
        if first == Some("agent-events") || first == Some("agent-event-state") {
            if code == UNREACHABLE {
                writeln!(err, "NorthThreads: {dest} unreachable; canonical events unavailable")?;
            }
            return Ok(Some(code));
        }
        if code == TIMED_OUT {
            return Ok(Some(code));
        }
        if code != UNREACHABLE {
            let calibrate_write = first == Some("calibrate") && has("--write");
            if !(code == 0 && calibrate_write && has("--goal-row")) {
                self.refresh_snapshot(dest, code == 0 && (first == Some("record-priors") || calibrate_write))?;
            }
            return Ok(Some(code));
        }
        if first.is_some_and(|verb| READS.contains(&verb)) && self.snapshot.exists() {
            writeln!(err, "{}", self.stale_line(dest)?)?;
            return Ok(None);
        }
        writeln!(err, "NorthThreads: {dest} unreachable; the ledger lives there, so nothing was read or recorded")?;
        Ok(Some(code))
    }

    fn stale_line(&self, dest: &str) -> NorthResult<String> {
        let modified = fs::metadata(&self.snapshot)?.modified()?;
        Ok(format!("STALE: ledger snapshot from {}; {dest} unreachable", Timestamp::from(modified).iso()))
    }

    fn spool_argv(&self, argv: &[String]) -> NorthResult<()> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.spool)?;
        fs::set_permissions(&self.spool, fs::Permissions::from_mode(0o700))?;
        let path = self.spool.join(format!("{}.argv", spool_stem()));
        let tmp = path.with_extension("tmp");
        OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?.write_all(python_json(argv).as_bytes())?;
        Ok(fs::rename(&tmp, &path)?)
    }

    fn flush_spool(&self, dest: &str, err: &mut dyn Write) -> NorthResult<Flush> {
        if !self.spool.is_dir() {
            return Ok(Flush::Clear);
        }
        let lock = File::create(self.spool.join(".lock"))?;
        lock.lock()?;
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.spool)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl" || extension == "argv"))
            .collect();
        paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        for path in paths {
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let code = if path.extension().is_some_and(|extension| extension == "argv") {
                let mut text = String::new();
                file.read_to_string(&mut text)?;
                let args: Vec<String> = serde_json::from_str(&text)?;
                let quiet = args.first().is_some_and(|verb| verb == "agent-event");
                self.ssh(dest, &args, Input::Null, quiet, err)?
            } else {
                self.ssh(dest, &["ingest".to_owned()], Input::File(file), false, err)?
            };
            if code == UNREACHABLE {
                return Ok(Flush::Down);
            }
            if code == ingest_lock::RETRY {
                return Ok(Flush::Busy);
            }
            if code != 0 {
                fs::rename(&path, path.with_extension("failed"))?;
                writeln!(err, "NorthThreads: remote ingest refused {}; kept as .failed", path.display())?;
                continue;
            }
            match fs::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(Flush::Clear)
    }

    fn refresh_snapshot(&self, dest: &str, force: bool) -> NorthResult<()> {
        let _ = snapshot::refresh(&self.pull_argv(dest), &self.snapshot, force, snapshot::PULL_TIMEOUT);
        Ok(())
    }

    fn pull_argv(&self, dest: &str) -> Vec<OsString> {
        self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, snapshot::REMOTE_COMMAND])
    }

    fn snapshot_verb(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        if argv[0] == "snapshot-pages" {
            if let Some(reason) = self.no_native_reason() {
                writeln!(err, "NorthThreads: the snapshot holds native items: {reason}")?;
                return Ok(1);
            }
            let database = self.var("THREADS_DB").map_or_else(|| self.home.join(".local/state/threads/threads.db"), PathBuf::from);
            return match snapshot::serve(&database, stdin, &mut io::stdout().lock()) {
                Ok(()) => Ok(0),
                Err(error) => {
                    writeln!(err, "NorthThreads: snapshot pages failed: {error}")?;
                    Ok(1)
                }
            };
        }
        let (mut force, mut path, mut dest) = (false, self.snapshot.clone(), None);
        let mut rest = argv[1..].iter();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--force" => force = true,
                "--snapshot" => path = rest.next().map(PathBuf::from).ok_or_else(|| NorthError::Usage("--snapshot needs a path".into()))?,
                _ if dest.is_none() && !arg.starts_with('-') => dest = Some(arg.clone()),
                _ => return Err(NorthError::Usage("usage: north threads snapshot-pull [--force] [--snapshot PATH] DEST".into())),
            }
        }
        let dest = dest.ok_or_else(|| NorthError::Usage("usage: north threads snapshot-pull [--force] [--snapshot PATH] DEST".into()))?;
        match snapshot::refresh(&self.pull_argv(&dest), &path, force, snapshot::PULL_TIMEOUT) {
            Ok(_) => Ok(0),
            Err(error) => {
                writeln!(err, "NorthThreads: snapshot pull from {dest} failed: {error}")?;
                Ok(1)
            }
        }
    }

    fn no_native_reason(&self) -> Option<&'static str> {
        if self.var("THREADS_NO_NATIVE").is_some() {
            return Some("THREADS_NO_NATIVE is set (agy)");
        }
        self.var("CLAUDE_CODE_REMOTE").is_some().then_some("CLAUDE_CODE_REMOTE is set (cloud worker)")
    }

    fn ssh(&self, dest: &str, args: &[String], input: Input, quiet: bool, err: &mut dyn Write) -> NorthResult<i32> {
        let mut remote = Vec::new();
        if let Some(actor) = self.var("THREADS_ACTOR") {
            remote.push(format!("THREADS_ACTOR={actor}"));
        }
        if self.no_native_reason().is_some() {
            remote.push("THREADS_NO_NATIVE=1".to_owned());
        }
        let first = args.first().map(String::as_str);
        if first == Some("unowned") {
            remote.push(format!("THREADS_ALIVE_OWNERS={}", python_json(&outside::fleet_owners(self.var("NORTH_BIN").unwrap_or("north")).into_iter().collect::<Vec<_>>())));
        }
        if first == Some("show") {
            remote.push("THREADS_ARCHIVES_LOCAL=1".to_owned());
        }
        if !remote.is_empty() {
            remote.insert(0, "env".to_owned());
        }
        if first == Some("ingest") {
            remote.push("north".to_owned());
        }
        remote.push("threads".to_owned());
        remote.extend(args.iter().cloned());
        let command = remote.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" ");
        let argv = self.ssh_argv(&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", dest, &command]);
        let stdout = if quiet { Stdio::null() } else { Stdio::inherit() };
        let wait = if first == Some("job-run") { self.record_wait } else { SSH_TIMEOUT };
        match execute(&argv, input, stdout, wait)? {
            Some(status) => Ok(code_of(status)),
            None => {
                writeln!(err, "NorthThreads: {dest} timed out; remote outcome unknown")?;
                Ok(TIMED_OUT)
            }
        }
    }

    fn ssh_argv(&self, arguments: &[&str]) -> Vec<OsString> {
        crate::platform::ssh::argv(&self.ssh, &self.env, arguments)
    }
}

fn agent_event_checks(argv: &[String]) -> Result<(), String> {
    let option = |name: &str| {
        let mut found = None;
        let prefix = format!("{name}=");
        for (index, arg) in argv.iter().enumerate() {
            if arg == name {
                found = argv.get(index + 1).cloned();
            } else if let Some(value) = arg.strip_prefix(&prefix) {
                found = Some(value.to_owned());
            }
        }
        found
    };
    if let Some(body) = option("--body") {
        outside::gitleaks_clean(&body)?;
    }
    if option("--key").is_some_and(|key| key.trim().is_empty()) {
        return Err("key must be nonempty text".into());
    }
    Ok(())
}

fn feed(mut child: Child, bytes: Option<Vec<u8>>) -> Child {
    if let (Some(bytes), Some(mut stdin)) = (bytes, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    child
}

fn execute(argv: &[OsString], input: Input, stdout: Stdio, wait: Duration) -> NorthResult<Option<ExitStatus>> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).stdout(stdout);
    let bytes = match input {
        Input::Inherit => None,
        Input::Null => {
            command.stdin(Stdio::null());
            None
        }
        Input::File(file) => {
            command.stdin(file);
            None
        }
        Input::Bytes(bytes) => {
            command.stdin(Stdio::piped());
            Some(bytes)
        }
    };
    let mut child = feed(command.spawn()?, bytes);
    let deadline = Instant::now() + wait;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Some(status))
}

fn code_of(status: ExitStatus) -> i32 {
    status.code().or_else(|| status.signal().map(|signal| -signal)).unwrap_or(1)
}

fn has_usage(data: &[u8]) -> NorthResult<bool> {
    for line in data.split(|byte| *byte == b'\n') {
        if line.trim_ascii().is_empty() {
            continue;
        }
        let row: Value = serde_json::from_slice(line)?;
        if row.get("type").and_then(Value::as_str) == Some("usage") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn spool_stem() -> String {
    format!("{}-{}", Timestamp::now().unix_nanos(), std::process::id())
}

pub(crate) fn shell_quote(word: &str) -> String {
    if word.is_empty() {
        return "''".into();
    }
    if word.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&byte)) {
        return word.into();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

fn python_json(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| {
        let mut text = String::from("\"");
        for unit in item.encode_utf16() {
            match unit {
                0x22 => text.push_str("\\\""),
                0x5c => text.push_str("\\\\"),
                0x0a => text.push_str("\\n"),
                0x0d => text.push_str("\\r"),
                0x09 => text.push_str("\\t"),
                0x08 => text.push_str("\\b"),
                0x0c => text.push_str("\\f"),
                0x20..=0x7e => text.push(unit as u8 as char),
                _ => text.push_str(&format!("\\u{unit:04x}")),
            }
        }
        text.push('"');
        text
    }).collect();
    format!("[{}]", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    struct Rig {
        _directory: tempfile::TempDir,
        root: PathBuf,
        client: Client,
    }

    const FAKE_SSH: &str = r#"for a; do last=$a; done
printf '%s\n' "$*" >> "$ROOT/ssh.log"
[ -f "$ROOT/sleep" ] && exec sleep "$(cat "$ROOT/sleep")"
case "$last" in
  *ingest) cat >> "$ROOT/ingest.log"; [ -f "$ROOT/ingest-code" ] && exit "$(cat "$ROOT/ingest-code")" ;;
  "north threads snapshot-pages") cat >/dev/null; cat "$ROOT/pages" ;;
esac
exit "$(cat "$ROOT/code")"
"#;

    fn rig(env: &[(&str, &str)]) -> Rig {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        fs::create_dir_all(root.join(".config/threads")).unwrap();
        fs::create_dir_all(root.join(".local/state/threads")).unwrap();
        fs::write(root.join(".config/threads/remote"), "ledger-host\n").unwrap();
        fs::write(root.join("code"), "0").unwrap();
        fs::write(root.join("fleet.json"), "[]\n").unwrap();
        let ledger = root.join("ledger.db");
        rusqlite::Connection::open(&ledger).unwrap().execute_batch("CREATE TABLE t(v); INSERT INTO t VALUES (1);").unwrap();
        let mut pages = Vec::new();
        snapshot::serve(&ledger, &mut &snapshot::manifest_bytes(0, &[])[..], &mut pages).unwrap();
        fs::write(root.join("pages"), pages).unwrap();
        let north = root.join("north");
        fs::write(&north, format!("#!/bin/sh\ncat '{}'\n", root.join("fleet.json").display())).unwrap();
        fs::set_permissions(&north, fs::Permissions::from_mode(0o755)).unwrap();
        let ssh = format!("ROOT='{}'\n{FAKE_SSH}", root.display());
        let threads = format!("printf '%s|%s\\n' \"$THREADS_DB\" \"$*\" >> '{}/threads.log'", root.display());
        let mut variables: BTreeMap<String, String> = env.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        variables.insert("HOME".into(), root.display().to_string());
        variables.insert("NORTH_BIN".into(), north.display().to_string());
        let client = Client::new(
            &root,
            variables,
            vec!["/bin/sh".into(), "-c".into(), ssh.into(), "ssh".into()],
            vec!["/bin/sh".into(), "-c".into(), threads.into(), "threads".into()],
        );
        Rig { _directory: directory, root, client }
    }

    fn call(rig: &Rig, argv: &[&str], stdin: &[u8]) -> (i32, String) {
        let argv: Vec<String> = argv.iter().map(|arg| arg.to_string()).collect();
        let mut err = Vec::new();
        let code = rig.client.main(&argv, &mut &stdin[..], &mut err).unwrap();
        (code, String::from_utf8(err).unwrap())
    }

    fn log(rig: &Rig, name: &str) -> Vec<String> {
        fs::read_to_string(rig.root.join(name)).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    const PREFIX: &str = "-o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=QUIET ledger-host";

    #[test]
    fn threads_client_runs_every_verb_over_ssh_with_the_same_env() {
        let rig = rig(&[("THREADS_ACTOR", "lead one"), ("CLAUDE_CODE_REMOTE", "1")]);
        fs::write(rig.root.join("fleet.json"), format!(
            r#"[{{"name": "w2", "status": "working", "provider": "codex", "conversation": "s9", "host_pid": {}}},
                {{"name": "gone", "status": "exited"}}, {{"name": "done", "status": "exited"}}]"#,
            std::process::id(),
        )).unwrap();
        let rig = rig;
        assert_eq!(call(&rig, &["show", "north#63"], b"").0, 0);
        assert_eq!(call(&rig, &["unowned"], b"").0, 0);
        assert_eq!(log(&rig, "ssh.log"), [
            format!("{PREFIX} env 'THREADS_ACTOR=lead one' THREADS_NO_NATIVE=1 THREADS_ARCHIVES_LOCAL=1 threads show 'north#63'"),
            format!("{PREFIX} north threads snapshot-pages"),
            format!("{PREFIX} env 'THREADS_ACTOR=lead one' THREADS_NO_NATIVE=1 'THREADS_ALIVE_OWNERS=[\"codex:s9\", \"s9\", \"w2\"]' threads unowned"),
        ]);
        assert!(fs::read(&rig.client.snapshot).unwrap().starts_with(b"SQLite format 3"));
    }

    #[test]
    fn threads_client_spools_unreachable_writes_and_flushes_them_in_order() {
        let rig = rig(&[]);
        fs::write(rig.root.join("code"), "255").unwrap();
        assert_eq!(call(&rig, &["job-run", "nightly", "--host", "laptop"], b"").0, 0);
        let (code, err) = call(&rig, &["ingest"], b"{\"type\": \"run\"}\n");
        assert_eq!(code, 0);
        assert!(err.contains("ledger-host unreachable; spooled ingest to "), "{err}");
        assert_eq!(fs::metadata(&rig.client.spool).unwrap().mode() & 0o777, 0o700);
        let packets: Vec<_> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "argv")).collect();
        assert_eq!(packets.len(), 1);
        assert_eq!(fs::metadata(&packets[0]).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::read_to_string(&packets[0]).unwrap(), r#"["job-run", "nightly", "--host", "laptop"]"#);
        let attempts = log(&rig, "ssh.log").len();
        fs::write(rig.root.join("code"), "0").unwrap();
        assert_eq!(call(&rig, &["list"], b"").0, 0);
        assert_eq!(log(&rig, "ssh.log")[attempts..], [
            format!("{PREFIX} threads job-run nightly --host laptop"),
            format!("{PREFIX} north threads ingest"),
            format!("{PREFIX} threads list"),
            format!("{PREFIX} north threads snapshot-pages"),
        ]);
        assert_eq!(log(&rig, "ingest.log"), ["{\"type\": \"run\"}"]);
        let left: Vec<_> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, [".lock"]);
    }

    #[test]
    fn threads_client_spools_a_job_run_record_the_ledger_host_does_not_answer_in_time_without_flushing_first() {
        let mut rig = rig(&[]);
        rig.client.record_wait = Duration::from_millis(300);
        fs::create_dir_all(&rig.client.spool).unwrap();
        fs::write(rig.client.spool.join("1-1.argv"), r#"["job-run", "earlier", "--host", "laptop"]"#).unwrap();
        fs::write(rig.root.join("sleep"), "30").unwrap();
        let begun = Instant::now();
        let (code, err) = call(&rig, &["job-run", "fleet-relay", "--host", "laptop", "--started", "s", "--finished", "f", "--rc", "0"], b"");
        assert!(begun.elapsed() < Duration::from_secs(5), "{:?}", begun.elapsed());
        assert_eq!((code, err.as_str()), (0, "NorthThreads: ledger-host timed out; remote outcome unknown\n"));
        assert_eq!(log(&rig, "ssh.log"), [format!("{PREFIX} threads job-run fleet-relay --host laptop --started s --finished f --rc 0")]);
        let mut packets: Vec<String> = fs::read_dir(&rig.client.spool).unwrap().map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "argv"))
            .map(|path| fs::read_to_string(path).unwrap()).collect();
        packets.sort();
        assert_eq!(packets, [
            r#"["job-run", "earlier", "--host", "laptop"]"#,
            r#"["job-run", "fleet-relay", "--host", "laptop", "--started", "s", "--finished", "f", "--rc", "0"]"#,
        ]);
    }

    #[test]
    fn queue_follow_refuses_a_follows_line_that_names_no_issue_sha_or_landed_subject() {
        let checkout = tempfile::tempdir().unwrap();
        let repo = checkout.path().join("north/main");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| assert!(Command::new("git").arg("-C").arg(&repo).args(args).status().unwrap().success(), "git {args:?}");
        git(&["init", "-q"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "Land the ledger fix"]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let rig = rig(&[("FOLLOWS_RELEASE_ROOT", checkout.path().to_str().unwrap())]);
        let queue = |follows: &str| call(&rig, &["queue", "follow", "north#9", "--tier", "sonnet", "--parent", "p"], format!("Item: north#9 x\nFollows: {follows}\n\nDo it.").as_bytes());
        let (code, err) = queue("the ledger fix");
        assert_eq!(code, 1);
        assert!(err.starts_with("NorthThreads: queue follow: Follows: the ledger fix names no issue, SHA or landed subject; use an issue (`repo#N` or `owner/repo#N`), a commit SHA"), "{err}");
        assert!(log(&rig, "ssh.log").is_empty(), "a refused box reaches no ledger");
        for accepted in ["north#240 (note)", "0123abc landed it", "Land the ledger fix"] {
            assert_eq!(queue(accepted).0, 0, "{accepted}");
        }
        assert_eq!(log(&rig, "ssh.log").iter().filter(|line| line.contains("queue follow 'north#9'")).count(), 3);
    }

    #[test]
    fn the_ledger_host_stores_a_follows_box_its_sender_vetted_without_vetting_it_against_its_own_checkouts() {
        let ledger = tempfile::tempdir().unwrap();
        let database = ledger.path().join("threads.db");
        rusqlite::Connection::open(&database).unwrap().execute_batch("CREATE TABLE job_queue(id INTEGER PRIMARY KEY, placement TEXT NOT NULL, item TEXT, cmd TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'queued', queued TEXT NOT NULL, taken TEXT, finished TEXT, result TEXT) STRICT").unwrap();
        let database = database.to_str().unwrap();
        let rig = rig(&[("THREADS_DB", database), (verb::SCHEMA_READY, "1"), ("FOLLOWS_RELEASE_ROOT", ledger.path().to_str().unwrap())]);
        let (code, err) = call(&rig, &["queue", "follow", "north#9", "--tier", "sonnet", "--parent", "p"], b"Item: north#9 x\nFollows: Land the ledger fix\n");
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(log(&rig, "ssh.log").is_empty());
        let stored: String = rusqlite::Connection::open(database).unwrap().query_row("SELECT item || ' ' || placement FROM job_queue", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, "north#9 follows");
    }

    #[test]
    fn threads_client_reads_fall_back_to_the_snapshot_with_the_stale_line() {
        let rig = rig(&[]);
        fs::write(rig.root.join("code"), "255").unwrap();
        fs::write(&rig.client.snapshot, "SQLite format 3\0").unwrap();
        File::options().write(true).open(&rig.client.snapshot).unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000)).unwrap();
        let (code, err) = call(&rig, &["ready"], b"");
        assert_eq!(code, 0);
        assert_eq!(err, "STALE: ledger snapshot from 2023-11-14T22:13:20Z; ledger-host unreachable\n");
        assert_eq!(log(&rig, "threads.log"), [format!("{}|ready", rig.client.snapshot.display())]);
        let (code, err) = call(&rig, &["claim", "north#63"], b"");
        assert_eq!(code, 255);
        assert_eq!(err, "NorthThreads: ledger-host unreachable; the ledger lives there, so nothing was read or recorded\n");
        assert_eq!(log(&rig, "threads.log").len(), 1);
    }

    #[test]
    fn a_caller_that_may_not_read_native_items_is_served_no_snapshot_pages() {
        let rig = rig(&[("THREADS_NO_NATIVE", "1")]);
        assert_eq!(call(&rig, &["snapshot-pages"], b""), (1, "NorthThreads: the snapshot holds native items: THREADS_NO_NATIVE is set (agy)\n".to_owned()));
    }

    fn ledger_host(wait: Duration) -> (tempfile::TempDir, Client) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/threads");
        let script = format!("echo start >> '{0}/order.log'; sleep 1; '{1}' \"$@\"; code=$?; echo end >> '{0}/order.log'; exit $code", root.display(), real.display());
        let north = std::env::current_exe().unwrap().parent().and_then(Path::parent).unwrap().join("north");
        let variables = BTreeMap::from([
            ("HOME".to_owned(), root.display().to_string()),
            ("THREADS_DB".to_owned(), root.join("threads.db").display().to_string()),
            ("NORTH_BIN".to_owned(), north.display().to_string()),
        ]);
        let mut client = Client::new(root, variables, vec!["ssh".into()], vec!["/bin/sh".into(), "-c".into(), script.into(), "threads".into()]);
        client.ingest_wait = wait;
        (directory, client)
    }

    fn batch(id: &str) -> Vec<u8> {
        format!(
            "{{\"id\":\"{id}\",\"role\":\"worker\",\"tier\":\"medium\",\"status\":\"finished\",\"outcome\":\"done\",\"started\":\"2026-10-26T11:00:00Z\",\"ended\":\"2026-10-26T11:05:00Z\",\"actual_min\":5,\"brief\":\"Item: smashcraft#459 x\\nCategory: feature\"}}\n"
        ).into_bytes()
    }

    fn recorded(root: &Path) -> Vec<String> {
        let db = rusqlite::Connection::open(root.join("threads.db")).unwrap();
        let mut statement = db.prepare("SELECT agent FROM runs ORDER BY agent").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn two_ingests_started_together_run_one_after_the_other_and_each_batch_is_recorded_once() {
        let (directory, client) = ledger_host(Duration::from_secs(30));
        let ingest = |id: &'static str| {
            let client = &client;
            move || {
                let mut err = Vec::new();
                let code = client.main(&["ingest".to_owned()], &mut &batch(id)[..], &mut err).unwrap();
                (code, String::from_utf8(err).unwrap())
            }
        };
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(ingest("one"));
            let second = scope.spawn(ingest("two"));
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!((first.0, second.0), (0, 0), "{first:?} {second:?}");
        let order = fs::read_to_string(directory.path().join("order.log")).unwrap();
        assert_eq!(order.lines().collect::<Vec<_>>(), ["start", "end", "start", "end"]);
        assert_eq!(recorded(directory.path()), ["one", "two"]);
    }

    #[test]
    fn an_ingest_that_cannot_get_the_lock_refuses_with_the_retry_code_and_records_nothing_until_retried() {
        let (directory, client) = ledger_host(Duration::from_millis(200));
        let held = ingest_lock::acquire(&directory.path().join("ingest.lock"), Duration::ZERO).unwrap().unwrap();
        let mut err = Vec::new();
        let code = client.main(&["ingest".to_owned()], &mut &batch("one")[..], &mut err).unwrap();
        assert_eq!(code, ingest_lock::RETRY);
        assert_eq!(String::from_utf8(err).unwrap(), "NorthThreads: another ingest holds the ledger; retry\n");
        assert!(!directory.path().join("threads.db").exists());
        drop(held);
        for _ in 0..2 {
            assert_eq!(client.main(&["ingest".to_owned()], &mut &batch("one")[..], &mut Vec::new()).unwrap(), 0);
        }
        assert_eq!(recorded(directory.path()), ["one"]);
    }

    #[test]
    fn a_busy_ledger_host_spools_the_batch_keeps_it_queued_and_stays_reachable_for_other_commands() {
        let rig = rig(&[]);
        let spooled = || {
            fs::read_dir(rig.root.join(".local/state/threads/spool")).unwrap()
                .filter_map(|entry| entry.unwrap().path().extension().map(|extension| extension.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
        };
        fs::write(rig.root.join("ingest-code"), ingest_lock::RETRY.to_string()).unwrap();
        let (code, err) = call(&rig, &["ingest"], b"{\"n\": 1}\n");
        assert_eq!(code, 0);
        assert!(err.contains("ledger-host busy; spooled ingest to "), "{err}");
        assert_eq!(spooled(), ["jsonl"]);
        let (code, err) = call(&rig, &["claim", "north#63"], b"");
        assert_eq!(code, 0, "a busy ingest lock is not an unreachable host: {err}");
        assert_eq!(spooled(), ["jsonl"], "a busy batch stays queued, never .failed");
        let (code, err) = call(&rig, &["ingest"], b"{\"n\": 2}\n");
        assert_eq!(code, 0);
        assert!(err.contains("spooled ingest to "), "{err}");
        assert_eq!(spooled(), ["jsonl", "jsonl"]);
        fs::remove_file(rig.root.join("ingest-code")).unwrap();
        assert_eq!(call(&rig, &["ingest"], b"{\"n\": 3}\n").0, 0);
        assert!(spooled().is_empty());
        let sent = log(&rig, "ingest.log");
        assert_eq!(&sent[sent.len() - 3..], ["{\"n\": 1}", "{\"n\": 2}", "{\"n\": 3}"]);
    }
}
