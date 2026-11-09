use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::time::Instant;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};

use super::config;
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

/// What one opencode host needs: the route its launcher chose, the scratch directory that holds its private data, the file that holds the paid key, and the words that start a confined `opencode` child.
pub struct Host {
    pub route: Route,
    pub session: String,
    pub scratch: PathBuf,
    pub key_file: Option<PathBuf>,
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
    tokens: [u64; 4],
    progressed: bool,
    error: Option<String>,
    interrupted: bool,
}

/// Serves the host protocol on `input` and `output`: a `user` line starts one `opencode run` turn in the session, an `interrupt` line stops the running one, and every opencode event is passed through between the `north_turn` frames that open and end the turn.
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
    let mut serving = Serving { host, events, output, queue: VecDeque::new(), turn: None, attempts: 0, closed: false };
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
        if self.closed {
            return Ok(false);
        }
        let Some((uuid, text)) = self.queue.pop_front() else { return Ok(true) };
        self.attempts += 1;
        let route = self.host.route;
        frame(self.output, &json!({"type": PHASE_FRAME, "phase": "start", "uuid": uuid, "sessionID": self.host.session, "text": text, "route": route.label(), "model": route.model(), "first": self.attempts == 1}))?;
        self.turn = Some(begin(self.host, &self.events, uuid, text, route, self.attempts)?);
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
            running.observe(line);
            writeln!(self.output, "{line}")?;
            self.output.flush()?;
        }
        Ok(())
    }

    fn done(&mut self, attempt: u64, code: Option<i32>) -> NorthResult<()> {
        let Some(mut running) = self.turn.take_if(|running| running.attempt == attempt) else { return Ok(()) };
        if running.error.is_none() && code != Some(0) && !running.interrupted {
            running.error = Some(format!("opencode exited {}", code.map_or_else(|| "on a signal".to_owned(), |code| code.to_string())));
        }
        let paid = running.route.fallback().filter(|_| running.error.is_some() && !running.progressed && !running.interrupted && self.host.key_file.is_some());
        let Some(route) = paid else { return frame(self.output, &running.end(self.host)) };
        self.attempts += 1;
        frame(self.output, &json!({"type": PHASE_FRAME, "phase": "fallback", "uuid": running.uuid, "sessionID": self.host.session, "from": running.route.label(), "to": route.label(), "reason": running.error}))?;
        self.turn = Some(begin(self.host, &self.events, running.uuid, running.text, route, self.attempts)?);
        Ok(())
    }
}

impl Turn {
    fn observe(&mut self, line: &str) {
        let Ok(event) = serde_json::from_str::<Value>(line) else { return };
        match event["type"].as_str() {
            Some("step_finish") => {
                let tokens = &event["part"]["tokens"];
                let count = |value: &Value| value.as_u64().unwrap_or(0);
                self.tokens[0] += count(&tokens["input"]);
                self.tokens[1] += count(&tokens["output"]) + count(&tokens["reasoning"]);
                self.tokens[2] += count(&tokens["cache"]["read"]);
                self.tokens[3] += count(&tokens["cache"]["write"]);
            }
            Some("text" | "tool_use") => self.progressed = true,
            Some("error") => self.error = Some(event["error"]["message"].as_str().or_else(|| event["error"]["type"].as_str()).unwrap_or("error").to_owned()),
            _ => {}
        }
    }

    fn end(&self, host: &Host) -> Value {
        let exit = if self.interrupted { "interrupted" } else if self.error.is_some() { "failed" } else { "completed" };
        let [input, output, cache_read, cache_write] = self.tokens;
        json!({
            "type": PHASE_FRAME, "phase": "end", "uuid": self.uuid, "sessionID": host.session, "exit": exit, "route": self.route.label(), "model": self.route.model(),
            "wall_ms": u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX), "error": self.error,
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
    let stdout = child.stdout.take().ok_or_else(|| NorthError::State("opencode started without a stdout".into()))?;
    let events = events.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = events.send(Event::Child(attempt, Output::Line(line)));
        }
        let code = child.wait().ok().and_then(|status| status.code());
        let _ = events.send(Event::Child(attempt, Output::Done(code)));
    });
    Ok(Turn { uuid, text, started: Instant::now(), route, attempt, pid, tokens: [0; 4], progressed: false, error: None, interrupted: false })
}

fn spawn(host: &Host, route: Route, text: &str) -> NorthResult<Child> {
    let mut words: Vec<String> = host.launcher.clone();
    words.push(host.program.display().to_string());
    words.extend(["run", "--standalone", "--format", "json", "--auto", "-m", route.model(), "-s", &host.session].map(str::to_owned));
    words.push(text.to_owned());
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
    command.env("OPENCODE_CONFIG_CONTENT", config::content(route).to_string());
    if route == Route::Go {
        let key = host.key_file.as_ref().ok_or_else(|| NorthError::Refused("the paid Go route has no key file".into()))?;
        command.env(config::KEY_ENV, std::fs::read_to_string(key)?.trim());
    }
    command.spawn().map_err(|error| NorthError::State(format!("{}: {error}", words[0])))
}

const USAGE: &str = "usage: north opencode-host --route go|free --session ses_ID --scratch DIR --program OPENCODE [--key-file FILE]";

impl Host {
    pub fn parse(arguments: &[String]) -> NorthResult<Self> {
        let usage = || NorthError::Usage(USAGE.into());
        let (mut route, mut session, mut scratch, mut program, mut key_file) = (None, None, None, None, None);
        let mut words = arguments.iter();
        while let Some(flag) = words.next() {
            let value = words.next().ok_or_else(usage)?;
            match flag.as_str() {
                "--route" => route = Route::parse(value),
                "--session" => session = Some(value.clone()),
                "--scratch" => scratch = Some(PathBuf::from(value)),
                "--program" => program = Some(PathBuf::from(value)),
                "--key-file" => key_file = Some(PathBuf::from(value)),
                _ => return Err(usage()),
            }
        }
        let cwd = std::env::current_dir()?;
        let launcher = [north_base::platform::north_binary::north_binary().display().to_string(), "--worker-sandbox".into(), "--lane".into(), "--".into()].into();
        Ok(Self { route: route.ok_or_else(usage)?, session: session.ok_or_else(usage)?, scratch: scratch.ok_or_else(usage)?, program: program.ok_or_else(usage)?, key_file, launcher, cwd })
    }
}

pub fn run(arguments: &[String]) -> NorthResult<u8> {
    let host = Host::parse(arguments)?;
    serve(&host, BufReader::new(std::io::stdin()), &mut std::io::stdout())?;
    Ok(0)
}
