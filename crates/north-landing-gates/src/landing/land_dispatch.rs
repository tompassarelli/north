use crate::foundation::carry;
use crate::platform::process;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: north land dispatch NAME (runs the command main's .safe-push declares under [dispatch \"NAME\"])";
const PREFIX: &str = "north land dispatch: ";
const LINE_LIMIT: u64 = 4096;
const GH_LIMIT: Duration = Duration::from_secs(60);
const APPEAR_SECONDS: u64 = 180;
const DEFAULT_DEADLINE: u64 = 3600;
const LONGEST_PAUSE: u64 = 15;
const MAIN: &str = "main";
const SET_BY_LANDER: [&str; 2] = ["ref", "base"];
const RUN_URL: &str = "/actions/runs/";

pub const INFRASTRUCTURE: u8 = north_base::foundation::exit::NO_VERDICT;

struct Declaration {
    command: String,
    workflow: String,
    reference: String,
    inputs: Vec<(String, Regex)>,
    deadline: Duration,
}

#[derive(Default)]
struct Ledger {
    dispatched: bool,
    found: bool,
    refused: Option<String>,
    missing: Option<String>,
}

struct Check {
    declaration: Declaration,
    head: String,
    base: String,
    result: PathBuf,
    ledger: Mutex<Ledger>,
    ended: AtomicBool,
}

pub fn run(arguments: &[String]) -> u8 {
    let [name] = arguments else {
        eprintln!("{USAGE}");
        return 64;
    };
    let check = match Check::new(Path::new("."), name) {
        Ok(check) => check,
        Err(why) => return infrastructure(&why),
    };
    let child = Command::new("bash").args(["-euo", "pipefail", "-c", &check.declaration.command]).env(carry::LAND_DISPATCHED, &check.result).stdin(Stdio::null()).stdout(Stdio::piped()).spawn();
    let status = match child {
        Ok(child) => check.watch(child),
        Err(error) => return infrastructure(&format!("cannot start {}: {error}", check.declaration.command)),
    };
    let _ = std::fs::remove_file(&check.result);
    check.verdict(status)
}

impl Check {
    fn new(directory: &Path, name: &str) -> Result<Self, String> {
        let declaration = main_declaration(directory, name)?;
        let revision = |variable: &str, fallback: &str| {
            let named = std::env::var(variable).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| fallback.to_owned());
            if named.is_empty() { Some(String::new()) } else { git(directory, &["rev-parse", "--verify", &format!("{named}^{{commit}}")]) }
        };
        let (Some(head), Some(base)) = (revision(carry::LAND_HEAD, "HEAD"), revision(carry::LAND_BASE, "")) else {
            return Err("the commit or base under check does not resolve".to_owned());
        };
        let git_dir = git(directory, &["rev-parse", "--absolute-git-dir"]).ok_or("the check runs outside a git checkout")?;
        let result = PathBuf::from(git_dir).join(format!("north-dispatch-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&result);
        Ok(Self { declaration, head, base, result, ledger: Mutex::default(), ended: AtomicBool::new(false) })
    }

    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn watch(&self, mut child: Child) -> std::io::Result<ExitStatus> {
        let Some(stdout) = child.stdout.take() else { return child.wait() };
        thread::scope(|scope| {
            let mut follower = None;
            let mut reader = BufReader::new(stdout);
            let mut out = std::io::stdout();
            loop {
                let mut line = Vec::new();
                match reader.by_ref().take(LINE_LIMIT + 1).read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let _ = out.write_all(&line);
                if !line.ends_with(b"\n") {
                    let _ = rest_of_line(&mut reader, &mut out);
                }
                if line.starts_with(PREFIX.as_bytes())
                    && let Some(inputs) = self.admitted(&line)
                {
                    follower = Some(scope.spawn(move || self.dispatch(&inputs)));
                }
            }
            let status = child.wait();
            self.ended.store(true, Ordering::SeqCst);
            if let Some(follower) = follower {
                let _ = follower.join();
            }
            status
        })
    }

    fn admitted(&self, line: &[u8]) -> Option<BTreeMap<String, String>> {
        let mut ledger = self.ledger();
        if ledger.dispatched || ledger.refused.is_some() {
            ledger.refused.get_or_insert_with(|| "the check asked for a second dispatch; a check dispatches once".to_owned());
            return None;
        }
        match accepted(&self.declaration, line) {
            Ok(inputs) => {
                ledger.dispatched = true;
                Some(inputs)
            }
            Err(why) => {
                write_state(&self.result, &json!({"state": "refused", "reason": why}));
                ledger.refused = Some(why);
                None
            }
        }
    }

    fn verdict(&self, status: std::io::Result<ExitStatus>) -> u8 {
        let ledger = self.ledger();
        if let Some(why) = &ledger.refused {
            return infrastructure(&format!("the dispatch was refused: {why}"));
        }
        if let Some(why) = &ledger.missing {
            return infrastructure(why);
        }
        if ledger.dispatched && !ledger.found {
            return infrastructure("the dispatched run never appeared");
        }
        exit_code(status)
    }

    fn missing(&self, why: String) {
        write_state(&self.result, &json!({"state": "failed", "reason": why}));
        self.ledger().missing = Some(why);
    }

    fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    fn dispatch(&self, inputs: &BTreeMap<String, String>) {
        match self.start(inputs) {
            Ok(id) => {
                self.ledger().found = true;
                self.follow(&id.to_string());
            }
            Err(why) => self.missing(why),
        }
    }

    fn start(&self, inputs: &BTreeMap<String, String>) -> Result<u64, String> {
        let workflow = self.declaration.workflow.as_str();
        let listing = ["run", "list", "--workflow", workflow, "--event", "workflow_dispatch", "--limit", "50", "--json", "databaseId,displayTitle,url"];
        let before: Vec<u64> = gh(&listing).ok().map(|runs| run_ids(&runs)).unwrap_or_default();
        let fields: Vec<String> = [("ref", self.head.as_str()), ("base", self.base.as_str())].into_iter().chain(inputs.iter().map(|(key, value)| (key.as_str(), value.as_str()))).map(|(key, value)| format!("{key}={value}")).collect();
        let mut arguments = vec!["workflow", "run", workflow, "--ref", self.declaration.reference.as_str()];
        for field in &fields {
            arguments.extend(["-f", field.as_str()]);
        }
        let printed = gh(&arguments).map_err(|why| format!("GitHub refused the dispatch of {workflow}: {why}"))?;
        let started = Instant::now();
        let mut id = run_id_in(&printed);
        while id.is_none() && started.elapsed() < Duration::from_secs(APPEAR_SECONDS) && !self.ended() {
            thread::sleep(Duration::from_secs(3));
            id = gh(&listing).ok().and_then(|runs| new_run(&runs, &before, &self.head));
        }
        id.ok_or_else(|| format!("no {workflow} run for {} appeared", self.head))
    }

    fn follow(&self, id: &str) {
        let started = Instant::now();
        let mut delay = 1;
        loop {
            if let Some(view) = gh(&["run", "view", id, "--json", "status,conclusion,url,jobs"]).ok().and_then(|view| serde_json::from_str::<Value>(&view).ok()) {
                let completed = text(&view, "status") == "completed";
                write_state(&self.result, &json!({"state": if completed { "completed" } else { "running" }, "run": run_state(id, &view)}));
                if completed {
                    let conclusion = text(&view, "conclusion");
                    if !matches!(conclusion, "success" | "failure") {
                        self.missing(format!("the run concluded {conclusion}, which judges nothing"));
                    }
                    return;
                }
            }
            if self.ended() {
                return;
            }
            if started.elapsed() >= self.declaration.deadline {
                let _ = gh(&["run", "cancel", id]);
                return self.missing(format!("the run did not finish within {} s; cancelled", self.declaration.deadline.as_secs()));
            }
            let pause = Instant::now();
            while pause.elapsed() < Duration::from_secs(delay) && !self.ended() {
                thread::sleep(Duration::from_millis(100));
            }
            delay = (delay * 2).min(LONGEST_PAUSE);
        }
    }
}

fn rest_of_line(reader: &mut impl BufRead, out: &mut impl Write) -> std::io::Result<()> {
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(());
        }
        let (taken, done) = buffer.iter().position(|byte| *byte == b'\n').map_or((buffer.len(), false), |at| (at + 1, true));
        out.write_all(&buffer[..taken])?;
        reader.consume(taken);
        if done {
            return Ok(());
        }
    }
}

fn exit_code(status: std::io::Result<ExitStatus>) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Ok(status) => status.code().map_or_else(|| 128 + status.signal().unwrap_or(0) as u8, |code| code as u8),
        Err(error) => infrastructure(&format!("the check's command could not be awaited: {error}")),
    }
}

fn main_declaration(directory: &Path, name: &str) -> Result<Declaration, String> {
    let fetched = Command::new("git").args(["fetch", "-q", "origin", MAIN]).current_dir(directory).status().is_ok_and(|status| status.success());
    let main = fetched.then(|| git(directory, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])).flatten().ok_or_else(|| format!("cannot fetch origin {MAIN}, whose .safe-push declares the dispatch"))?;
    let listed = git(directory, &["config", "--blob", &format!("{main}:.safe-push"), "--get-regexp", &format!("^dispatch\\.{}\\.", regex::escape(name))]).unwrap_or_default();
    declaration(name, &listed)
}

fn declaration(name: &str, listed: &str) -> Result<Declaration, String> {
    let prefix = format!("dispatch.{name}.");
    let mut values: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in listed.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if let Some(field) = key.strip_prefix(&prefix) {
            values.entry(field).or_default().push(value);
        }
    }
    let one = |field: &str| values.get(field).and_then(|all| all.last()).filter(|value| !value.is_empty()).map(|value| (*value).to_owned());
    let missing = |field: &str| format!("main's .safe-push declares no dispatch.{name}.{field}");
    let inputs = values.get("input").into_iter().flatten().map(|entry| input(name, entry)).collect::<Result<Vec<_>, _>>()?;
    let deadline = one("deadline").map_or(Ok(DEFAULT_DEADLINE), |seconds| seconds.parse()).map_err(|_| format!("dispatch.{name}.deadline is not a number of seconds"))?;
    Ok(Declaration {
        command: one("command").ok_or_else(|| missing("command"))?,
        workflow: one("workflow").ok_or_else(|| missing("workflow"))?,
        reference: one("ref").unwrap_or_else(|| MAIN.to_owned()),
        inputs,
        deadline: Duration::from_secs(deadline),
    })
}

fn input(name: &str, entry: &str) -> Result<(String, Regex), String> {
    let (key, pattern) = entry.split_once(char::is_whitespace).ok_or_else(|| format!("dispatch.{name}.input {entry} names no pattern"))?;
    if SET_BY_LANDER.contains(&key) || key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)) {
        return Err(format!("dispatch.{name}.input may not declare {key}"));
    }
    let pattern = Regex::new(&format!("^(?:{})$", pattern.trim())).map_err(|error| format!("dispatch.{name}.input {key}: {error}"))?;
    Ok((key.to_owned(), pattern))
}

fn accepted(declaration: &Declaration, line: &[u8]) -> Result<BTreeMap<String, String>, String> {
    if line.len() as u64 > LINE_LIMIT || !line.ends_with(b"\n") {
        return Err(format!("the dispatch line is longer than {LINE_LIMIT} bytes"));
    }
    let text = std::str::from_utf8(&line[PREFIX.len()..]).map_err(|_| "the dispatch line is not UTF-8".to_owned())?;
    let fields: Map<String, Value> = serde_json::from_str(text.trim_end()).map_err(|error| format!("the dispatch line is not a JSON object: {error}"))?;
    let mut inputs = BTreeMap::new();
    for (key, value) in fields {
        let Some(value) = value.as_str() else { return Err(format!("input {key} is not a string")) };
        let Some((_, pattern)) = declaration.inputs.iter().find(|(allowed, _)| *allowed == key) else { return Err(format!("main's declaration does not allow input {key}")) };
        if !pattern.is_match(value) {
            return Err(format!("input {key} does not match its declared pattern"));
        }
        inputs.insert(key, value.to_owned());
    }
    Ok(inputs)
}

fn gh(arguments: &[&str]) -> Result<String, String> {
    let verb = arguments.iter().take(2).copied().collect::<Vec<_>>().join(" ");
    let output = process::run_within("gh", arguments, GH_LIMIT).ok_or_else(|| format!("gh {verb} timed out"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(format!("gh {verb}: {}", String::from_utf8_lossy(&output.stderr).trim()))
}

fn run_state(id: &str, view: &Value) -> Value {
    let jobs: Vec<Value> = view.get("jobs").and_then(Value::as_array).into_iter().flatten().map(|job| json!({"name": text(job, "name"), "status": text(job, "status"), "conclusion": text(job, "conclusion")})).collect();
    json!({"id": id, "url": text(view, "url"), "status": text(view, "status"), "conclusion": text(view, "conclusion"), "jobs": jobs})
}

fn write_state(result: &Path, state: &Value) {
    let staged = result.with_extension("part");
    if std::fs::write(&staged, state.to_string()).and_then(|()| std::fs::rename(&staged, result)).is_err() {
        eprintln!("land dispatch: cannot write {}", result.display());
    }
}

fn run_id_in(printed: &str) -> Option<u64> {
    let (_, after) = printed.split_once(RUN_URL)?;
    after.split(|character: char| !character.is_ascii_digit()).next()?.parse().ok()
}

fn run_ids(runs: &str) -> Vec<u64> {
    let runs: Value = serde_json::from_str(runs).unwrap_or_default();
    runs.as_array().into_iter().flatten().filter_map(|run| run.get("databaseId").and_then(Value::as_u64)).collect()
}

fn new_run(runs: &str, before: &[u64], head: &str) -> Option<u64> {
    let runs: Value = serde_json::from_str(runs).ok()?;
    runs.as_array()?.iter().filter(|run| text(run, "displayTitle").contains(head)).filter_map(|run| run.get("databaseId").and_then(Value::as_u64)).find(|id| !before.contains(id))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).current_dir(directory).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn infrastructure(why: &str) -> u8 {
    eprintln!("land dispatch: {why}; no verdict");
    INFRASTRUCTURE
}
