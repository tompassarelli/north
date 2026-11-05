use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fs::{FlockOperation, flock};
use serde_json::Value;

use super::die::Die;
use super::stamp::{iso, parse, write_atomic};
use super::state::State;
use super::zone::Window;
use crate::platform::process::run_within;
use crate::shell::shlex_split;

const FORWARD_ENV: [&str; 2] = ["LAPTOP_GRACE", "LAPTOP_BY"];
pub(super) const FORWARD_WAIT: Duration = Duration::from_secs(90);
const THREADS_WAIT: Duration = Duration::from_secs(60);

pub struct Ctx {
    pub(super) state: PathBuf,
    pub(super) window: PathBuf,
    now: Option<String>,
    by: Option<String>,
    pub(super) grace: Option<String>,
    threads: Vec<String>,
    note_item: Option<String>,
    remote: PathBuf,
    threads_db: bool,
    env: BTreeMap<String, String>,
    forward_wait: Duration,
    ssh: Vec<std::ffi::OsString>,
}

pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}

impl Ctx {
    pub fn from_env() -> Self {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        let set = |name: &str| env.get(name).filter(|value| !value.is_empty()).cloned();
        let home = PathBuf::from(set("HOME").unwrap_or_default());
        Self {
            state: set("LAPTOP_STATE").map_or_else(|| home.join(".local/state/placement/laptop.json"), PathBuf::from),
            window: set("LAPTOP_WINDOW_FILE").map_or_else(|| PathBuf::from("/etc/placement/laptop-window.json"), PathBuf::from),
            now: set("LAPTOP_NOW"),
            by: set("LAPTOP_BY"),
            grace: set("LAPTOP_GRACE"),
            threads: shlex_split(&set("LAPTOP_THREADS").unwrap_or_else(|| "threads".to_owned())).unwrap_or_default(),
            note_item: set("LAPTOP_NOTE_ITEM"),
            remote: home.join(".config/threads/remote"),
            threads_db: set("THREADS_DB").is_some(),
            env,
            forward_wait: FORWARD_WAIT,
            ssh: vec!["ssh".into()],
        }
    }

    pub fn with_by(mut self, by: &str) -> Self {
        self.by = Some(by.to_owned());
        self
    }

    pub(super) fn now(&self) -> Result<i64, Die> {
        match &self.now {
            Some(text) => parse(text),
            None => Ok(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64)),
        }
    }

    pub(super) fn actor(&self) -> String {
        self.by.clone().unwrap_or_else(|| format!("{}@{}", self.env.get("USER").map_or("unknown", String::as_str), hostname()))
    }

    pub(super) fn forwarding(&self) -> bool {
        self.remote.exists() && !self.threads_db
    }

    pub(super) fn lock(&self) -> Result<File, Die> {
        let path = self.state.with_extension("lock");
        let io = |error: std::io::Error| Die::new(1, format!("{}: {error}", path.display()));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).map_err(io)?;
        flock(&file, FlockOperation::LockExclusive).map_err(|error| Die::new(1, format!("{}: {error}", path.display())))?;
        Ok(file)
    }

    pub(super) fn forward(&self, argv: &[String]) -> Result<String, Die> {
        let destination = fs::read_to_string(&self.remote).unwrap_or_default().trim().to_owned();
        let mut env: Vec<String> = FORWARD_ENV.iter().filter_map(|name| Some(format!("{name}={}", self.env.get(*name).filter(|value| !value.is_empty())?))).collect();
        if !env.iter().any(|entry| entry.starts_with("LAPTOP_BY=")) {
            env.push(format!("LAPTOP_BY={}", self.actor()));
        }
        let mut words = vec!["env".to_owned()];
        words.extend(env);
        words.extend(["north".to_owned(), "laptop".to_owned()]);
        words.extend(argv.iter().cloned());
        let command = words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" ");
        let options = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", &destination, &command];
        let ssh = crate::platform::ssh::argv(&self.ssh, &self.env, &options);
        let words: Vec<String> = ssh.iter().map(|word| word.to_string_lossy().into_owned()).collect();
        let refs: Vec<&str> = words[1..].iter().map(String::as_str).collect();
        let Some(output) = run_within(&words[0], &refs, self.forward_wait) else {
            return Err(Die::new(124, format!("{destination} timed out; remote outcome unknown")));
        };
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout);
        }
        print!("{stdout}");
        Err(Die::silent(output.status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1)))
    }

    pub(super) fn load(&self) -> Option<State> {
        serde_json::from_str(&fs::read_to_string(&self.state).ok()?).ok()
    }

    pub(super) fn sibling(&self, name: &str) -> PathBuf {
        self.state.with_file_name(name)
    }

    pub(super) fn threads(&self, args: &[&str]) -> Option<String> {
        let (program, fixed) = self.threads.split_first()?;
        let words: Vec<&str> = fixed.iter().map(String::as_str).chain(args.iter().copied()).collect();
        let output = run_within(program, &words, THREADS_WAIT)?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn note_item(&self) -> Option<String> {
        if let Some(item) = &self.note_item {
            return Some(item.clone());
        }
        let path = self.sibling("ledger-item");
        if let Ok(text) = fs::read_to_string(&path) {
            return Some(text.trim().to_owned());
        }
        let created = self.threads(&["new", "ops", "Laptop availability"]).filter(|out| !out.trim().is_empty())?;
        fs::write(&path, format!("{}\n", created.trim())).ok()?;
        Some(created.trim().to_owned())
    }

    pub(super) fn write(&self, state: &str, at: i64, reason: &str, by: &str, until: Option<i64>, grace: Option<i64>) -> Result<State, Die> {
        let written = State { state: state.to_owned(), since: iso(at), reason: reason.to_owned(), by: by.to_owned(), until: until.map(iso), grace };
        let text = serde_json::to_string(&written).map_err(|error| Die::new(1, error.to_string()))?;
        write_atomic(&self.state, &format!("{text}\n"))?;
        let note = format!("laptop: {}", self.describe(&written));
        if self.note_item().is_none_or(|item| self.threads(&["note", &item, &note]).is_none()) {
            eprintln!("laptop: ledger note failed; the state is written");
        }
        Ok(written)
    }

    pub(super) fn describe(&self, state: &State) -> String {
        let window = Window::read(&self.window);
        let hhmm = |text: &str| parse(text).map_or_else(|_| text.to_owned(), |unix| window.zone.hhmm(unix));
        let mut out = format!("{} since {}", state.state, hhmm(&state.since));
        if let Some(until) = &state.until {
            out.push_str(&format!(" until {}", hhmm(until)));
        }
        out.push_str(&format!(", {} by {}", state.reason, state.by));
        out
    }

    pub(super) fn jobs(&self) -> Option<Jobs> {
        let rows = serde_json::from_str::<Value>(&self.threads(&["queue", "--json"])?).ok()?.as_array()?.clone();
        let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        let me = hostname();
        let running = rows
            .iter()
            .filter(|row| text(row, "state") == "taken")
            .filter(|row| {
                let host = text(row, "host");
                text(row, "placement") == "laptop" || (!host.is_empty() && host != me)
            })
            .count();
        let queued: Vec<&Value> = rows.iter().filter(|row| matches!(text(row, "state").as_str(), "queued" | "requeued") && text(row, "placement") == "laptop").collect();
        let waiting = queued.iter().map(|row| text(row, "waiting")).find(|reason| !reason.is_empty());
        Some(Jobs { running, queued: queued.len(), waiting })
    }
}

pub(super) struct Jobs {
    pub(super) running: usize,
    pub(super) queued: usize,
    pub(super) waiting: Option<String>,
}

fn quote(word: &str) -> String {
    if !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"@%+=:,./-_".contains(&byte)) {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
impl Ctx {
    pub fn fixture(root: &std::path::Path, now: &str, threads: &str) -> Self {
        Self {
            state: root.join("placement/laptop.json"),
            window: root.join("window.json"),
            now: Some(now.to_owned()),
            by: Some("tester@host".to_owned()),
            grace: None,
            threads: shlex_split(threads).unwrap_or_default(),
            note_item: Some("x".to_owned()),
            remote: root.join("no-remote"),
            threads_db: false,
            env: BTreeMap::new(),
            forward_wait: FORWARD_WAIT,
            ssh: vec!["ssh".into()],
        }
    }
}

#[cfg(test)]
impl Ctx {
    pub fn stalled(mut self, ssh: &std::path::Path, wait: Duration) -> Self {
        self.ssh = vec![ssh.into()];
        self.forward_wait = wait;
        self
    }
}
