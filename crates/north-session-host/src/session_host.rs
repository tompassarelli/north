use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::error::{NorthError, NorthResult};

mod frame_watch;
mod handover;
mod pipes;
mod provider;

pub use frame_watch::{FrameWatch, SentLog};

use pipes::{append, copy_lines, copy_rotating, feed_stdin};
use provider::{Provider, signal};

pub const STDERR_ROTATE_BYTES: u64 = 10 * 1024 * 1024;
pub const STDIN_LINES: usize = 64;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const PARTIAL_LINE_TIMEOUT: Duration = Duration::from_secs(5);
pub const STDIN_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const REST_TICK: Duration = Duration::from_secs(5);
const TERM_GRACE: Duration = Duration::from_secs(10);
pub const STDIN_BLOCKED: &str = r#"{"error":"stdin-blocked"}"#;
pub const STDIN_CLOSED: &str = r#"{"error":"stdin-closed"}"#;
pub const INVALID_FRAME: &str = r#"{"error":"invalid-frame"}"#;

const USAGE: &str = "Usage: north session-host --name NAME --generation N --state-dir DIR [--socket PATH] [--idle-exit SECS] [--provider NAME] \
     [--adopt FILE] -- RUNTIME [ARG...]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostConfig {
    pub name: String,
    pub generation: u64,
    pub state_dir: PathBuf,
    pub socket: Option<PathBuf>,
    pub idle_exit: Option<Duration>,
    pub provider: Option<String>,
    pub adopt: Option<PathBuf>,
    pub runtime: Vec<String>,
}

impl HostConfig {
    pub fn parse(arguments: &[String]) -> NorthResult<Self> {
        let usage = || NorthError::Usage(USAGE.into());
        let (mut name, mut generation, mut state_dir, mut socket, mut idle_exit) = (None, None, None, None, None);
        let (mut provider, mut adopt) = (None, None);
        let mut index = 0;
        while index < arguments.len() {
            let flag = arguments[index].as_str();
            if flag == "--" {
                break;
            }
            let value = arguments.get(index + 1).filter(|value| !value.is_empty()).ok_or_else(usage)?;
            match flag {
                "--name" if name.is_none() => name = Some(value.clone()),
                "--generation" if generation.is_none() => {
                    generation = Some(value.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(usage)?)
                }
                "--state-dir" if state_dir.is_none() => state_dir = Some(PathBuf::from(value)),
                "--socket" if socket.is_none() => socket = Some(PathBuf::from(value)),
                "--provider" if provider.is_none() => provider = Some(value.clone()),
                "--adopt" if adopt.is_none() => adopt = Some(PathBuf::from(value)),
                "--idle-exit" if idle_exit.is_none() => {
                    let seconds = value.parse::<u64>().map_err(|_| usage())?;
                    idle_exit = Some((seconds > 0).then(|| Duration::from_secs(seconds)));
                }
                _ => return Err(usage()),
            }
            index += 2;
        }
        let runtime = arguments.get(index + 1..).unwrap_or_default().to_vec();
        match (name, generation, state_dir) {
            (Some(name), Some(generation), Some(state_dir)) if !runtime.is_empty() => Ok(Self {
                name,
                generation,
                state_dir,
                socket,
                idle_exit: idle_exit.flatten(),
                provider,
                adopt,
                runtime,
            }),
            _ => Err(usage()),
        }
    }

    pub fn generation_dir(&self) -> PathBuf {
        self.state_dir.join(format!("g{}", self.generation))
    }

    pub fn socket_path(&self) -> NorthResult<PathBuf> {
        if let Some(socket) = &self.socket {
            return Ok(socket.clone());
        }
        Ok(north_base::platform::session_host_files::socket_path(&self.state_dir))
    }
}

pub type Reply = mpsc::UnboundedSender<String>;

struct Frame {
    line: String,
    reply: Option<Reply>,
}

#[derive(Clone)]
pub struct Activity(Arc<Mutex<Instant>>);

impl Activity {
    fn since(idle: Duration) -> Self {
        Self(Arc::new(Mutex::new(Instant::now().checked_sub(idle).unwrap_or_else(Instant::now))))
    }

    fn touch(&self) {
        *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }

    fn idle_for(&self) -> Duration {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).elapsed()
    }
}

#[derive(Clone)]
pub struct StdinQueue {
    control: mpsc::Sender<Frame>,
    user: mpsc::Sender<Frame>,
    next: Arc<Mutex<u64>>,
    activity: Activity,
    watcher: Watcher,
}

/// The receiving ends of a host's two stdin lanes: control frames, then user frames.
struct Lanes {
    control: mpsc::Receiver<Frame>,
    user: mpsc::Receiver<Frame>,
}

impl StdinQueue {
    fn open(next: u64, activity: Activity, watcher: Watcher) -> (Self, Lanes) {
        let (control, control_lane) = mpsc::channel(STDIN_LINES);
        let (user, user_lane) = mpsc::channel(STDIN_LINES);
        (Self { control, user, next: Arc::new(Mutex::new(next)), activity, watcher }, Lanes { control: control_lane, user: user_lane })
    }

    fn sequence(&self) -> u64 {
        *self.next.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub async fn enqueue(&self, line: String, reply: Option<Reply>) -> Option<u64> {
        self.activity.touch();
        let respond = |message: String| {
            if let Some(reply) = &reply {
                let _ = reply.send(message);
            }
        };
        let lane = if is_control(&line) { &self.control } else { &self.user };
        let permit = match tokio::time::timeout(STDIN_WRITE_TIMEOUT, lane.reserve()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                respond(STDIN_CLOSED.into());
                return None;
            }
            Err(_) => {
                respond(STDIN_BLOCKED.into());
                return None;
            }
        };
        self.watcher.sent(&line);
        let mut next = self.next.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *next += 1;
        respond(format!(r#"{{"host_seq":{}}}"#, *next));
        permit.send(Frame { line, reply: reply.clone() });
        Some(*next)
    }
}

fn is_control(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .is_ok_and(|frame| frame.get("type").and_then(|kind| kind.as_str()) == Some("control_request"))
}

pub enum Rest {
    Wait,
    Send(Vec<String>),
    End,
}

/// A host's handle on its frame watch: every question it asks takes the watch's lock, and a frame it sends is logged
/// through the call the watch handed out, which never takes that lock.
#[derive(Clone)]
pub struct Watcher {
    watch: Arc<Mutex<FrameWatch>>,
    sent: SentLog,
}

impl Default for Watcher {
    fn default() -> Self {
        Self::new(FrameWatch::default())
    }
}

impl Watcher {
    pub fn new(watch: FrameWatch) -> Self {
        Self { sent: watch.sent_log(), watch: Arc::new(Mutex::new(watch)) }
    }

    fn lock(&self) -> MutexGuard<'_, FrameWatch> {
        self.watch.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lines(&self, bytes: &[u8]) {
        let mut watch = self.lock();
        for line in bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()) {
            watch.line(line);
        }
    }

    fn sent(&self, line: &str) {
        self.sent.sent(line);
    }

    fn output_keeps_awake(&self) -> bool {
        self.lock().in_flight()
    }

    fn soonest_rest(&self) -> Option<Duration> {
        self.lock().soonest_rest()
    }

    fn rest(&self, idle: Duration, park_after: Option<Duration>) -> Rest {
        self.lock().rest(idle, park_after)
    }

    fn close(&self) {
        self.lock().close();
    }

    fn hand_over(&self) -> serde_json::Value {
        self.lock().hand_over()
    }

    fn take_over(&self, state: serde_json::Value) {
        self.lock().take_over(state);
    }
}

/// How far a host has wound down to hand its session to a new image: serving, no longer taking clients or resting,
/// then stopping its pipe tasks at a frame and line boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Serving,
    Closing,
    Handing,
}

struct Pipes {
    input: ChildStdin,
    output: Option<ChildStdout>,
    errors: Option<ChildStderr>,
}

struct Fed {
    input: ChildStdin,
    lanes: Lanes,
}

pub struct SessionHost {
    pub stdin: StdinQueue,
    directory: PathBuf,
    provider: Provider,
    writer: JoinHandle<Fed>,
    stdout: JoinHandle<io::Result<Option<ChildStdout>>>,
    stderr: JoinHandle<io::Result<Option<ChildStderr>>>,
    server: Option<JoinHandle<Option<UnixListener>>>,
    parking: Option<JoinHandle<()>>,
    park_after: Option<Duration>,
    watcher: Watcher,
    stage: watch::Sender<Stage>,
}

impl SessionHost {
    #[cfg(test)]
    pub fn spawn(config: &HostConfig) -> NorthResult<Self> {
        Self::spawn_watched(config, Watcher::default())
    }

    pub fn spawn_watched(config: &HostConfig, watcher: Watcher) -> NorthResult<Self> {
        let mut child = std::process::Command::new(&config.runtime[0])
            .args(&config.runtime[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let missing = || NorthError::State("runtime pipe was not captured".into());
        let pipes = Pipes {
            input: ChildStdin::from_std(child.stdin.take().ok_or_else(missing)?)?,
            output: Some(ChildStdout::from_std(child.stdout.take().ok_or_else(missing)?)?),
            errors: Some(ChildStderr::from_std(child.stderr.take().ok_or_else(missing)?)?),
        };
        let (stdin, lanes) = StdinQueue::open(0, Activity::since(Duration::ZERO), watcher.clone());
        Ok(Self::hold(config.generation_dir(), Provider::watch(child.id()), pipes, (stdin, lanes), watcher)?)
    }

    fn hold(directory: PathBuf, provider: Provider, pipes: Pipes, (stdin, lanes): (StdinQueue, Lanes), watcher: Watcher) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        let stream = append(&directory.join("stream.jsonl"))?;
        let log = directory.join("stderr.log");
        let (stage, _) = watch::channel(Stage::Serving);
        let stdout = match pipes.output {
            Some(output) => tokio::spawn(copy_lines(output, stream, stdin.activity.clone(), watcher.clone(), stage.subscribe())),
            None => tokio::spawn(async { Ok(None) }),
        };
        let stderr = match pipes.errors {
            Some(errors) => tokio::spawn(copy_rotating(errors, log, STDERR_ROTATE_BYTES, stage.subscribe())),
            None => tokio::spawn(async { Ok(None) }),
        };
        Ok(Self {
            writer: tokio::spawn(feed_stdin(pipes.input, lanes, stage.subscribe())),
            stdout,
            stderr,
            stdin,
            directory,
            provider,
            server: None,
            parking: None,
            park_after: None,
            watcher,
            stage,
        })
    }

    pub fn serve(&mut self, listener: UnixListener) {
        self.server = Some(tokio::spawn(accept_clients(listener, self.stdin.clone(), self.stage.subscribe())));
    }

    pub fn end_with_host_signal(&self) {
        let pid = self.provider.pid;
        let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) else { return };
        tokio::spawn(async move {
            term.recv().await;
            signal(pid, rustix::process::Signal::Term);
            tokio::time::sleep(TERM_GRACE).await;
            signal(pid, rustix::process::Signal::Kill);
        });
    }

    pub fn rest_when_idle(&mut self, park_after: Option<Duration>) {
        self.park_after = park_after;
        let resting = rest_when_idle(self.stdin.clone(), self.watcher.clone(), park_after, self.provider.pid, self.stage.subscribe());
        self.parking = Some(tokio::spawn(resting));
    }

    pub async fn wait(self) -> NorthResult<u8> {
        let Self { stdin, mut provider, writer, stdout, stderr, server, parking, watcher, .. } = self;
        drop(stdin);
        provider.ended().await;
        let code = provider.reap();
        server.iter().for_each(JoinHandle::abort);
        parking.iter().for_each(JoinHandle::abort);
        writer.abort();
        let _ = writer.await;
        let drained = (stdout.await.map(|copied| copied.map(drop)), stderr.await.map(|copied| copied.map(drop)));
        watcher.close();
        for result in [drained.0, drained.1] {
            result.map_err(|error| NorthError::State(format!("pipe task failed: {error}")))??;
        }
        Ok(code)
    }
}

pub async fn run(arguments: &[String], watch_for: impl FnOnce(&HostConfig) -> Option<FrameWatch>) -> NorthResult<u8> {
    let config = HostConfig::parse(arguments)?;
    let socket = config.socket_path()?;
    fs::create_dir_all(&config.state_dir)?;
    fs::write(north_base::platform::session_host_files::pid_path(&config.state_dir), std::process::id().to_string())?;
    let watcher = watch_for(&config).map(Watcher::new).unwrap_or_default();
    let mut host = match &config.adopt {
        Some(carried) => handover::adopt(&config, carried, watcher)?,
        None => {
            let listener = bind(&socket)?;
            let mut host = SessionHost::spawn_watched(&config, watcher)?;
            host.serve(listener);
            host
        }
    };
    host.end_with_host_signal();
    host.rest_when_idle(config.idle_exit);
    let code = handover::follow(host, &config).await;
    let _ = fs::remove_file(&socket);
    code
}

async fn rest_when_idle(stdin: StdinQueue, watcher: Watcher, park_after: Option<Duration>, child: u32, mut stage: watch::Receiver<Stage>) {
    let tick = park_after.into_iter().chain(watcher.soonest_rest()).map(|limit| limit / 4).fold(REST_TICK, Duration::min);
    loop {
        tokio::select! {
            biased;
            Ok(_) = stage.wait_for(|stage| *stage != Stage::Serving) => return,
            () = tokio::time::sleep(tick) => {}
        }
        match watcher.rest(stdin.activity.idle_for(), park_after) {
            Rest::Wait => {}
            Rest::Send(frames) => {
                for frame in frames {
                    stdin.enqueue(frame, None).await;
                }
            }
            Rest::End => break,
        }
    }
    signal(child, rustix::process::Signal::Term);
    tokio::time::sleep(TERM_GRACE).await;
    signal(child, rustix::process::Signal::Kill);
}

pub fn bind(path: &Path) -> io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    UnixListener::bind(path)
}

/// Serves clients until the host winds down, then gives any still connected `DEFER` to finish and returns the
/// listener for the host's next image.
async fn accept_clients(listener: UnixListener, queue: StdinQueue, mut stage: watch::Receiver<Stage>) -> Option<UnixListener> {
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            Ok(_) = stage.wait_for(|stage| *stage != Stage::Serving) => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { return None };
                while clients.try_join_next().is_some() {}
                clients.spawn(serve_client(stream, queue.clone()));
            }
        }
    }
    let finished = tokio::time::timeout(north_runtime_follow::DEFER, async { while clients.join_next().await.is_some() {} }).await;
    if finished.is_err() {
        clients.shutdown().await;
    }
    Some(listener)
}

async fn serve_client(stream: UnixStream, queue: StdinQueue) {
    let (mut reader, mut writer) = stream.into_split();
    let (reply, mut replies) = mpsc::unbounded_channel::<String>();
    let responder = tokio::spawn(async move {
        while let Some(mut message) = replies.recv().await {
            message.push('\n');
            if writer.write_all(message.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let detached = read_frames(&mut reader, &queue, &reply).await.unwrap_or(false);
    drop(reply);
    if detached {
        let _ = responder.await;
    } else {
        responder.abort();
    }
}

async fn read_frames(
    reader: &mut (impl AsyncRead + Unpin),
    queue: &StdinQueue,
    reply: &Reply,
) -> io::Result<bool> {
    let mut pending = Vec::new();
    let mut since = None;
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = match since {
            Some(start) => {
                let deadline = start + PARTIAL_LINE_TIMEOUT;
                match tokio::time::timeout_at(deadline, reader.read(&mut chunk)).await {
                    Ok(read) => read?,
                    Err(_) => return Ok(false),
                }
            }
            None => reader.read(&mut chunk).await?,
        };
        if read == 0 {
            return Ok(true);
        }
        let start = pending.len();
        pending.extend_from_slice(&chunk[..read]);
        let mut consumed = 0;
        while let Some(position) = pending[consumed.max(start)..].iter().position(|byte| *byte == b'\n') {
            let end = consumed.max(start) + position;
            if end - consumed > MAX_FRAME_BYTES {
                return Ok(false);
            }
            match std::str::from_utf8(&pending[consumed..end]) {
                Ok(line) if serde_json::from_str::<serde_json::Value>(line).is_ok() => {
                    queue.enqueue(line.to_owned(), Some(reply.clone())).await;
                }
                _ => {
                    let _ = reply.send(INVALID_FRAME.into());
                }
            }
            consumed = end + 1;
        }
        pending.drain(..consumed);
        if pending.len() > MAX_FRAME_BYTES {
            return Ok(false);
        }
        since = match (pending.is_empty(), consumed > 0) {
            (true, _) => None,
            (false, true) => Some(Instant::now()),
            (false, false) => since.or_else(|| Some(Instant::now())),
        };
    }
}

#[cfg(test)]
mod socket {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader, Lines};
    use tokio::net::unix::OwnedReadHalf;
    use tokio::net::unix::OwnedWriteHalf;

    struct Client {
        replies: Lines<BufReader<OwnedReadHalf>>,
        frames: OwnedWriteHalf,
    }

    impl Client {
        async fn connect(path: &Path) -> Self {
            let (reader, frames) = UnixStream::connect(path).await.unwrap().into_split();
            Self { replies: BufReader::new(reader).lines(), frames }
        }

        async fn write(&mut self, bytes: &str) {
            self.frames.write_all(bytes.as_bytes()).await.unwrap();
        }

        async fn reply(&mut self) -> Option<String> {
            self.replies.next_line().await.unwrap()
        }

        async fn send(&mut self, frame: &str) -> String {
            self.write(&format!("{frame}\n")).await;
            self.reply().await.unwrap()
        }
    }

    fn host(root: &Path, script: &str) -> (SessionHost, PathBuf) {
        let config = HostConfig {
            name: "lead".into(),
            generation: 1,
            state_dir: root.to_path_buf(),
            socket: Some(root.join("lead-g1.sock")),
            idle_exit: None,
            provider: None,
            adopt: None,
            runtime: vec!["sh".into(), "-c".into(), script.into()],
        };
        let path = config.socket_path().unwrap();
        let listener = bind(&path).unwrap();
        let mut host = SessionHost::spawn(&config).unwrap();
        host.serve(listener);
        (host, path)
    }

    async fn stream_lines(root: &Path, count: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = fs::read_to_string(root.join("g1/stream.jsonl")).unwrap_or_default();
            let lines = text.lines().map(String::from).collect::<Vec<_>>();
            if lines.len() >= count || Instant::now() > deadline {
                return lines;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn user(n: usize) -> String {
        format!(r#"{{"type":"user","n":{n}}}"#)
    }

    #[tokio::test]
    async fn control_frame_overtakes_queued_user_frames() {
        let root = tempfile::tempdir().unwrap();
        let (host, path) = host(root.path(), "sleep 2; exec head -n 12");
        let mut client = Client::connect(&path).await;
        let filler = format!(r#"{{"type":"user","pad":"{}"}}"#, "x".repeat(512 * 1024));
        let control = r#"{"type":"control_request","request_id":"r1","request":{"subtype":"interrupt"}}"#;
        assert!(client.send(&filler).await.contains("host_seq"));
        for n in 1..=10 {
            assert!(client.send(&user(n)).await.contains("host_seq"));
        }
        assert!(client.send(control).await.contains("host_seq"));
        assert_eq!(host.wait().await.unwrap(), 0);
        let lines = stream_lines(root.path(), 12).await;
        let control_at = lines.iter().position(|line| line == control).unwrap();
        let first_user = lines.iter().position(|line| *line == user(1)).unwrap();
        assert!(control_at < first_user, "{control_at} {first_user}");
    }
}

