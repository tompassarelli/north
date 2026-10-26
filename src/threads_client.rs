use std::collections::{BTreeMap, BTreeSet};
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
use crate::threads::{claims, snapshot};
use crate::foundation::clock::Timestamp;

const READS: [&str; 12] = [
    "job-runs", "show", "ready", "list", "unowned", "run-ids", "summary", "recommend", "calibrate",
    "replay-staffing", "planner-band", "routing-compare",
];
const UNREACHABLE: i32 = 255;
const TIMED_OUT: i32 = 124;
const SSH_TIMEOUT: Duration = Duration::from_secs(20);
const RECORD_WAIT: Duration = Duration::from_secs(10);

pub(crate) struct Client {
    home: PathBuf,
    remote: PathBuf,
    spool: PathBuf,
    snapshot: PathBuf,
    env: BTreeMap<String, String>,
    ssh: Vec<OsString>,
    threads: Vec<OsString>,
    record_wait: Duration,
}

enum Input {
    Inherit,
    Null,
    Bytes(Vec<u8>),
    File(File),
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let env: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let Some(home) = env.get("HOME").cloned() else {
        eprintln!("NorthThreads: no home directory");
        return 1;
    };
    let client = Client::new(Path::new(&home), env, vec!["ssh".into()], vec!["threads".into()]);
    match client.main(arguments, &mut io::stdin(), &mut io::stderr()) {
        Ok(code) => (code & 0xff) as u8,
        Err(error) => {
            eprintln!("NorthThreads: {error}");
            1
        }
    }
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
        }
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.env.get(name).map(String::as_str).filter(|value| !value.is_empty())
    }

    fn main(&self, argv: &[String], stdin: &mut dyn Read, err: &mut dyn Write) -> NorthResult<i32> {
        let first = argv.first().map(String::as_str);
        if first == Some("snapshot-pages") || first == Some("snapshot-pull") {
            return self.snapshot_verb(argv, stdin, err);
        }
        let database = self.var("THREADS_DB").is_some();
        if first == Some("routing-compare") && !database {
            return self.local(argv, Some(&self.snapshot));
        }
        if !database && self.remote.exists() {
            let dest = fs::read_to_string(&self.remote)?.trim().to_owned();
            return match self.proxy(&dest, argv, stdin, err)? {
                Some(code) => Ok(code),
                None => self.local(argv, Some(&self.snapshot)),
            };
        }
        self.local(argv, None)
    }

    fn local(&self, argv: &[String], database: Option<&Path>) -> NorthResult<i32> {
        if argv.first().is_some_and(|verb| claims::verbs::VERBS.contains(&verb.as_str())) {
            let ledger = database.map(Path::to_path_buf).or_else(|| self.var("THREADS_DB").map(PathBuf::from));
            let ledger = ledger.unwrap_or_else(|| self.home.join(".local/state/threads/threads.db"));
            let actor = self.var("THREADS_ACTOR").or_else(|| self.var("USER")).unwrap_or("unknown").to_owned();
            let context = claims::verbs::Context { database: &ledger, actor, no_native: self.no_native_reason(), issues: &claims::issues::GitHub, now: Timestamp::now().unix_seconds() };
            return Ok(claims::verbs::run(argv, &context, &mut io::stdout().lock(), &mut io::stderr()));
        }
        let mut command = Command::new(&self.threads[0]);
        command.args(&self.threads[1..]).args(argv);
        if let Some(database) = database {
            command.env("THREADS_DB", database);
        }
        Ok(code_of(command.status()?))
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
        let reachable = self.flush_spool(dest, err)?;
        if first == Some("ingest") {
            let mut data = Vec::new();
            stdin.read_to_end(&mut data)?;
            if reachable {
                let code = self.ssh(dest, &argv, Input::Bytes(data.clone()), false, err)?;
                if code != UNREACHABLE {
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
            writeln!(err, "NorthThreads: {dest} unreachable; spooled ingest to {}", path.display())?;
            return Ok(Some(0));
        }
        if first == Some("agent-event") {
            let code = if reachable { self.ssh(dest, &argv, Input::Null, false, err)? } else { UNREACHABLE };
            if code != UNREACHABLE {
                return Ok(Some(code));
            }
            self.spool_argv(&argv)?;
            return Ok(Some(0));
        }
        let code = if reachable { self.ssh(dest, &argv, Input::Inherit, false, err)? } else { UNREACHABLE };
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

    fn flush_spool(&self, dest: &str, err: &mut dyn Write) -> NorthResult<bool> {
        if !self.spool.is_dir() {
            return Ok(true);
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
                return Ok(false);
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
        Ok(true)
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
            remote.push(format!("THREADS_ALIVE_OWNERS={}", python_json(&self.local_alive_owners())));
        }
        if first == Some("show") {
            remote.push("THREADS_ARCHIVES_LOCAL=1".to_owned());
        }
        if !remote.is_empty() {
            remote.insert(0, "env".to_owned());
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
        let mut argv = self.ssh.clone();
        let configured = self.env.get("NORTH_SSH_CONFIG");
        let profile = configured.map_or_else(
            || PathBuf::from(self.env.get("HOME").map_or("", String::as_str)).join(".config/threads/ssh-config"),
            PathBuf::from,
        );
        if configured.is_some() || profile.is_file() {
            argv.extend(["-F".into(), profile.into_os_string()]);
        }
        if let Some(runtime) = self.var("XDG_RUNTIME_DIR")
            && runtime.starts_with('/')
            && runtime.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
            && runtime.len() + "/north-ssh-".len() + 40 + 20 <= 107
        {
            for option in [
                "ControlMaster=auto".to_owned(), format!("ControlPath={runtime}/north-ssh-%C"),
                "ControlPersist=10m".to_owned(), "ServerAliveInterval=5".to_owned(), "ServerAliveCountMax=2".to_owned(),
            ] {
                argv.extend(["-o".into(), option.into()]);
            }
        }
        argv.extend(arguments.iter().map(OsString::from));
        argv
    }

    fn local_alive_owners(&self) -> Vec<String> {
        let north = self.var("NORTH_BIN").unwrap_or("north");
        let Ok(output) = Command::new(north).args(["fleet", "list", "--json"]).output() else {
            return Vec::new();
        };
        if !output.status.success() { return Vec::new(); }
        let Some(sessions) = serde_json::from_slice::<Value>(&output.stdout).ok().and_then(|value| value.as_array().cloned()) else {
            return Vec::new();
        };
        let mut owners = BTreeSet::new();
        for session in sessions.iter().filter(|session| session.get("status").and_then(Value::as_str) != Some("exited")) {
            let text = |key: &str| session.get(key).and_then(Value::as_str).filter(|value| !value.is_empty());
            owners.extend(text("name").map(str::to_owned));
            owners.extend(text("conversation").map(str::to_owned));
            if let (Some(provider), Some(conversation)) = (text("provider"), text("conversation")) {
                owners.insert(format!("{provider}:{conversation}"));
            }
        }
        owners.into_iter().collect()
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
        gitleaks_clean(&body)?;
    }
    if option("--key").is_some_and(|key| key.trim().is_empty()) {
        return Err("key must be nonempty text".into());
    }
    Ok(())
}

fn gitleaks_clean(text: &str) -> Result<(), String> {
    let argv: Vec<OsString> = ["gitleaks", "stdin", "--no-banner", "--redact", "-l", "error"].map(OsString::from).to_vec();
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).current_dir("/").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err("gitleaks is not on PATH; items are scanned for secrets before they are stored".into());
        }
        Err(error) => return Err(error.to_string()),
    };
    let output = feed(child, Some(text.as_bytes().to_vec())).wait_with_output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored".into());
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

fn shell_quote(word: &str) -> String {
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
  *ingest) cat >> "$ROOT/ingest.log" ;;
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
            format!("{PREFIX} threads ingest"),
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
}
