use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::time::Instant;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};

use super::harness::{self, Harness, Tally};
use super::route::Route;

pub const PHASE_FRAME: &str = "north_turn";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Ready,
    Start,
    Fallback,
    End,
}

impl Phase {
    /// The phase a `north_turn` frame of this host reports; none for an opencode event.
    pub fn of(frame: &Value) -> Option<Self> {
        if frame["type"] != PHASE_FRAME {
            return None;
        }
        [("ready", Self::Ready), ("start", Self::Start), ("fallback", Self::Fallback), ("end", Self::End)].into_iter().find(|(label, _)| frame["phase"] == *label).map(|(_, phase)| phase)
    }
}

/// What one opencode host needs: the route its launcher chose, the effort it runs at, the scratch directory that holds its private data, the file that holds the paid key, and the words that start a confined child of the harness.
pub struct Host {
    pub harness: &'static dyn Harness,
    pub route: Route,
    pub effort: Option<String>,
    pub session: String,
    pub scratch: PathBuf,
    pub key_file: Option<PathBuf>,
    pub instructions: Option<PathBuf>,
    pub launcher: Vec<String>,
    pub program: PathBuf,
    pub cwd: PathBuf,
}

enum Event {
    Input(Value),
    Closed,
    Child(u64, Output),
}

enum Output {
    Line(String),
    Done(Option<i32>),
}

struct Turn {
    uuid: String,
    text: String,
    started: Instant,
    route: Route,
    attempt: u64,
    pid: u32,
    tally: Tally,
    interrupted: bool,
}

/// Serves the host protocol on `input` and `output`: a `user` line starts one turn of the harness in the session, an `interrupt` line stops the running one, and every event of the harness is passed through between the `north_turn` frames that open and end the turn.
pub fn serve(host: &Host, input: impl BufRead + Send + 'static, output: &mut impl Write) -> NorthResult<()> {
    let (events, inbox) = channel();
    let reader = events.clone();
    std::thread::spawn(move || {
        for line in input.lines().map_while(Result::ok) {
            if let Ok(frame) = serde_json::from_str::<Value>(&line) {
                let _ = reader.send(Event::Input(frame));
            }
        }
        let _ = reader.send(Event::Closed);
    });
    frame(output, &json!({"type": PHASE_FRAME, "phase": "ready", "sessionID": host.session}))?;
    let mut serving = Serving { host, route: host.route, events, output, queue: VecDeque::new(), turn: None, attempts: 0, closed: false };
    loop {
        if serving.turn.is_none() && !serving.next()? {
            return Ok(());
        }
        match inbox.recv().map_err(|error| NorthError::State(error.to_string()))? {
            Event::Input(message) => serving.input(&message),
            Event::Closed => serving.closed = true,
            Event::Child(attempt, Output::Line(line)) => serving.line(attempt, &line)?,
            Event::Child(attempt, Output::Done(code)) => serving.done(attempt, code)?,
        }
    }
}

struct Serving<'a, W> {
    host: &'a Host,
    route: Route,
    events: Sender<Event>,
    output: &'a mut W,
    queue: VecDeque<(String, String)>,
    turn: Option<Turn>,
    attempts: u64,
    closed: bool,
}

impl<W: Write> Serving<'_, W> {
    /// Starts the next queued turn; false once the input has closed and nothing is left to run.
    fn next(&mut self) -> NorthResult<bool> {
        let Some((uuid, text)) = self.queue.pop_front() else { return Ok(!self.closed) };
        self.attempts += 1;
        let route = self.route;
        frame(self.output, &json!({"type": PHASE_FRAME, "phase": "start", "uuid": uuid, "sessionID": self.host.session, "text": text, "route": route.label(), "model": route.model(), "effort": self.host.effort.as_deref(), "first": self.attempts == 1}))?;
        let sent = match self.host.instructions.as_ref().filter(|_| self.attempts == 1) {
            Some(file) => format!("{}\n\n{text}", std::fs::read_to_string(file)?.trim_end()),
            None => text,
        };
        self.turn = Some(begin(self.host, &self.events, uuid, sent, route, self.attempts)?);
        Ok(true)
    }

    fn input(&mut self, message: &Value) {
        match message["type"].as_str() {
            Some("user") => self.queue.push_back((message["uuid"].as_str().unwrap_or_default().to_owned(), message["text"].as_str().unwrap_or_default().to_owned())),
            Some("interrupt") => {
                if let Some(running) = self.turn.as_mut() {
                    running.interrupted = true;
                    stop(running.pid);
                }
            }
            _ => {}
        }
    }

    fn line(&mut self, attempt: u64, line: &str) -> NorthResult<()> {
        if let Some(running) = self.turn.as_mut().filter(|running| running.attempt == attempt) {
            self.host.harness.read(&mut running.tally, line);
            writeln!(self.output, "{}", with_session(line, &self.host.session))?;
            self.output.flush()?;
        }
        Ok(())
    }

    fn done(&mut self, attempt: u64, code: Option<i32>) -> NorthResult<()> {
        let Some(mut running) = self.turn.take_if(|running| running.attempt == attempt) else { return Ok(()) };
        if running.tally.error.is_none() && code != Some(0) && !running.interrupted {
            running.tally.error = Some(format!("{} exited {}", self.host.harness.provider(), code.map_or_else(|| "on a signal".to_owned(), |code| code.to_string())));
        }
        let paid = running.route.fallback().filter(|_| running.tally.error.is_some() && !running.tally.progressed && !running.interrupted && self.host.key_file.is_some());
        let Some(route) = paid else { return frame(self.output, &running.end(self.host)) };
        self.attempts += 1;
        self.route = route;
        frame(self.output, &json!({"type": PHASE_FRAME, "phase": "fallback", "uuid": running.uuid, "sessionID": self.host.session, "from": running.route.label(), "to": route.label(), "reason": running.tally.error}))?;
        self.turn = Some(begin(self.host, &self.events, running.uuid, running.text, route, self.attempts)?);
        Ok(())
    }
}

impl Turn {
    fn end(&self, host: &Host) -> Value {
        let exit = if self.interrupted { "interrupted" } else if self.tally.error.is_some() { "failed" } else { "completed" };
        let [input, output, cache_read, cache_write] = self.tally.tokens;
        json!({
            "type": PHASE_FRAME, "phase": "end", "uuid": self.uuid, "sessionID": host.session, "exit": exit, "route": self.route.label(), "model": self.route.model(),
            "effort": host.effort.as_deref(),
            "wall_ms": u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX), "error": self.tally.error,
            "tokens": {"input": input, "output": output, "cache_read": cache_read, "cache_write": cache_write},
        })
    }
}

fn frame(output: &mut impl Write, value: &Value) -> NorthResult<()> {
    writeln!(output, "{value}")?;
    output.flush()?;
    Ok(())
}

fn stop(pid: u32) {
    if let Some(pid) = i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::Term);
    }
}

fn begin(host: &Host, events: &Sender<Event>, uuid: String, text: String, route: Route, attempt: u64) -> NorthResult<Turn> {
    let mut child = spawn(host, route, &text)?;
    let pid = child.id();
    let stdout = child.stdout.take().ok_or_else(|| NorthError::State(format!("{} started without a stdout", host.harness.provider())))?;
    let events = events.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = events.send(Event::Child(attempt, Output::Line(line)));
        }
        let code = child.wait().ok().and_then(|status| status.code());
        let _ = events.send(Event::Child(attempt, Output::Done(code)));
    });
    Ok(Turn { uuid, text, started: Instant::now(), route, attempt, pid, tally: Tally::default(), interrupted: false })
}

fn spawn(host: &Host, route: Route, text: &str) -> NorthResult<Child> {
    let mut words: Vec<String> = host.launcher.clone();
    words.push(host.program.display().to_string());
    words.extend(host.harness.words(&host.session, route, host.effort.as_deref(), text));
    let mut command = Command::new(&words[0]);
    command.args(&words[1..]).current_dir(&host.cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).env_clear();
    for name in ["PATH", "HOME", "LANG", "TERM", "USER", "TMPDIR", "SHELL", "CARGO_HOME", "RUSTUP_HOME", "NORTH_FLEET_SESSION", "NORTH_FLEET_ROLE", "THREADS_ACTOR", "AGENT_CAPACITY_NEED"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.envs(crate::platform::git_credential::session_env());
    for (variable, directory) in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache")] {
        let path = host.scratch.join(directory);
        std::fs::create_dir_all(&path)?;
        command.env(variable, path);
    }
    std::fs::create_dir_all(host.scratch.join("tmp"))?;
    command.env("TMPDIR", host.scratch.join("tmp")).env("NORTH_SANDBOX_SCRATCH", &host.scratch);
    command.env("SHELL", command_shell(host));
    let key = match route {
        Route::Go => {
            let file = host.key_file.as_ref().ok_or_else(|| NorthError::Refused("the paid Go route has no key file".into()))?;
            Some(std::fs::read_to_string(file)?.trim().to_owned())
        }
        Route::Free => None,
    };
    harness::apply(&mut command, host.harness.environment(host, route, key.as_deref()));
    command.spawn().map_err(|error| NorthError::State(format!("{}: {error}", words[0])))
}

/// The shell the harness's shell tool runs each command through: a fresh lane-only domain asked of the broker. The turn's own lane-only domain was built before any lane the turn creates, so its Landlock rules cannot cover that lane; the broker leaves it for a domain built when the command starts (north#1033).
fn command_shell(host: &Host) -> PathBuf {
    let path = host.scratch.join("shell");
    let north = north_base::platform::north_binary::north_binary();
    let script = format!("exec env SHELL=/bin/sh {} --worker-sandbox --outside -- {} --worker-sandbox --lane -- /bin/sh \"$@\"\n", north.display(), north.display());
    north_base::foundation::shim::write_script(&path, &script);
    path
}

fn usage(harness: &dyn Harness) -> String {
    format!("usage: north {} --route go|free --session ID --scratch DIR --program PROGRAM [--key-file FILE] [--instructions FILE] [--effort high|max]", harness.host_verb())
}

/// The event line with the host's session id added when the agent names none, so every frame of a harness that keeps its session id to itself still reaches the fleet under the conversation.
fn with_session(line: &str, session: &str) -> String {
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(mut event)) if !event.contains_key("sessionID") => {
            event.insert("sessionID".into(), session.into());
            Value::Object(event).to_string()
        }
        _ => line.to_owned(),
    }
}

impl Host {
    pub fn parse(harness: &'static dyn Harness, arguments: &[String]) -> NorthResult<Self> {
        let usage = || NorthError::Usage(usage(harness));
        let (mut route, mut session, mut scratch, mut program, mut key_file, mut instructions, mut effort) = (None, None, None, None, None, None, None);
        let mut words = arguments.iter();
        while let Some(flag) = words.next() {
            let value = words.next().ok_or_else(usage)?;
            match flag.as_str() {
                "--route" => route = Route::parse(value),
                "--session" => session = Some(value.clone()),
                "--scratch" => scratch = Some(PathBuf::from(value)),
                "--program" => program = Some(PathBuf::from(value)),
                "--key-file" => key_file = Some(PathBuf::from(value)),
                "--instructions" => instructions = Some(PathBuf::from(value)),
                "--effort" => effort = Some(value.clone()),
                _ => return Err(usage()),
            }
        }
        let cwd = std::env::current_dir()?;
        let launcher = [north_base::platform::north_binary::north_binary().display().to_string(), "--worker-sandbox".into(), "--lane".into(), "--".into()].into();
        Ok(Self { harness, route: route.ok_or_else(usage)?, effort, session: session.ok_or_else(usage)?, scratch: scratch.ok_or_else(usage)?, program: program.ok_or_else(usage)?, key_file, instructions, launcher, cwd })
    }
}

pub fn run(harness: &'static dyn Harness, arguments: &[String]) -> NorthResult<u8> {
    let host = Host::parse(harness, arguments)?;
    serve(&host, BufReader::new(std::io::stdin()), &mut std::io::stdout())?;
    Ok(0)
}
