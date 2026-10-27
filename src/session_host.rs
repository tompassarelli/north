use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::error::{NorthError, NorthResult};

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
     -- RUNTIME [ARG...]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostConfig {
    pub name: String,
    pub generation: u64,
    pub state_dir: PathBuf,
    pub socket: Option<PathBuf>,
    pub idle_exit: Option<Duration>,
    pub provider: Option<String>,
    pub runtime: Vec<String>,
}

impl HostConfig {
    pub fn parse(arguments: &[String]) -> NorthResult<Self> {
        let usage = || NorthError::Usage(USAGE.into());
        let (mut name, mut generation, mut state_dir, mut socket, mut idle_exit) = (None, None, None, None, None);
        let mut provider = None;
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
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| NorthError::State("XDG_RUNTIME_DIR is not set".into()))?;
        Ok(PathBuf::from(runtime)
            .join("north")
            .join(format!("{}-g{}.sock", self.name, self.generation)))
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
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
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
}

impl StdinQueue {
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

pub trait FrameWatch: Send {
    fn line(&mut self, line: &[u8]);
    fn in_flight(&self) -> bool;
    fn rest(&mut self, idle: Duration, park_after: Option<Duration>) -> Rest;
    fn close(&mut self);
}

#[derive(Clone, Default)]
pub struct Watcher(Option<Arc<Mutex<Box<dyn FrameWatch>>>>);

impl Watcher {
    pub fn new(watch: Box<dyn FrameWatch>) -> Self {
        Self(Some(Arc::new(Mutex::new(watch))))
    }

    fn with<T>(&self, idle: T, act: impl FnOnce(&mut Box<dyn FrameWatch>) -> T) -> T {
        match &self.0 {
            Some(watch) => act(&mut watch.lock().unwrap_or_else(|poisoned| poisoned.into_inner())),
            None => idle,
        }
    }

    fn lines(&self, bytes: &[u8]) {
        self.with((), |watch| bytes.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()).for_each(|line| watch.line(line)));
    }

    fn output_keeps_awake(&self) -> bool {
        self.with(true, |watch| watch.in_flight())
    }

    fn rest(&self, idle: Duration, park_after: Option<Duration>) -> Rest {
        let parked = if park_after.is_some_and(|limit| idle >= limit) { Rest::End } else { Rest::Wait };
        self.with(parked, |watch| watch.rest(idle, park_after))
    }

    fn close(&self) {
        self.with((), |watch| watch.close());
    }
}

pub struct SessionHost {
    pub stdin: StdinQueue,
    child: Child,
    writer: JoinHandle<ChildStdin>,
    stdout: JoinHandle<io::Result<()>>,
    stderr: JoinHandle<io::Result<()>>,
    server: Option<JoinHandle<()>>,
    parking: Option<JoinHandle<()>>,
    watcher: Watcher,
}

impl SessionHost {
    #[cfg(test)]
    pub fn spawn(config: &HostConfig) -> NorthResult<Self> {
        Self::spawn_watched(config, Watcher::default())
    }

    pub fn spawn_watched(config: &HostConfig, watcher: Watcher) -> NorthResult<Self> {
        let directory = config.generation_dir();
        fs::create_dir_all(&directory)?;
        let stream = append(&directory.join("stream.jsonl"))?;
        let log = directory.join("stderr.log");
        let mut child = Command::new(&config.runtime[0])
            .args(&config.runtime[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let missing = || NorthError::State("runtime pipe was not captured".into());
        let input = child.stdin.take().ok_or_else(missing)?;
        let output = child.stdout.take().ok_or_else(missing)?;
        let errors = child.stderr.take().ok_or_else(missing)?;
        let (control, priority) = mpsc::channel(STDIN_LINES);
        let (user, lines) = mpsc::channel(STDIN_LINES);
        let activity = Activity::new();
        Ok(Self {
            stdin: StdinQueue { control, user, next: Arc::default(), activity: activity.clone() },
            child,
            writer: tokio::spawn(feed_stdin(input, priority, lines)),
            stdout: tokio::spawn(copy_lines(output, stream, activity, watcher.clone())),
            stderr: tokio::spawn(copy_rotating(errors, log, STDERR_ROTATE_BYTES)),
            server: None,
            parking: None,
            watcher,
        })
    }

    pub fn serve(&mut self, listener: UnixListener) {
        self.server = Some(tokio::spawn(accept_clients(listener, self.stdin.clone())));
    }

    pub fn end_with_host_signal(&self) {
        let Some(child) = self.child.id() else { return };
        let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) else { return };
        tokio::spawn(async move {
            term.recv().await;
            let Some(pid) = i32::try_from(child).ok().and_then(rustix::process::Pid::from_raw) else { return };
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::Term);
            tokio::time::sleep(TERM_GRACE).await;
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::Kill);
        });
    }

    pub fn rest_when_idle(&mut self, park_after: Option<Duration>) {
        let resting = rest_when_idle(self.stdin.clone(), self.watcher.clone(), park_after, self.child.id());
        self.parking = Some(tokio::spawn(resting));
    }

    pub async fn wait(self) -> NorthResult<u8> {
        let Self { stdin, mut child, writer, stdout, stderr, server, parking, watcher } = self;
        drop(stdin);
        let status = child.wait().await?;
        for task in [server, parking].into_iter().flatten() {
            task.abort();
        }
        writer.abort();
        let _ = writer.await;
        let drained = (stdout.await, stderr.await);
        watcher.close();
        for result in [drained.0, drained.1] {
            result.map_err(|error| NorthError::State(format!("pipe task failed: {error}")))??;
        }
        Ok(exit_code(status))
    }
}

pub fn exit_code(status: ExitStatus) -> u8 {
    match (status.code(), status.signal()) {
        (Some(code), _) => u8::try_from(code).unwrap_or(1),
        (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(255),
        (None, None) => 1,
    }
}

pub async fn run(arguments: &[String], watch_for: impl FnOnce(&HostConfig) -> Option<Box<dyn FrameWatch>>) -> NorthResult<u8> {
    let config = HostConfig::parse(arguments)?;
    let socket = config.socket_path()?;
    fs::create_dir_all(&config.state_dir)?;
    fs::write(config.state_dir.join("host.pid"), std::process::id().to_string())?;
    let listener = bind(&socket)?;
    let watcher = watch_for(&config).map(Watcher::new).unwrap_or_default();
    let mut host = SessionHost::spawn_watched(&config, watcher)?;
    host.serve(listener);
    host.end_with_host_signal();
    host.rest_when_idle(config.idle_exit);
    let code = host.wait().await;
    let _ = fs::remove_file(&socket);
    code
}

async fn rest_when_idle(stdin: StdinQueue, watcher: Watcher, park_after: Option<Duration>, child: Option<u32>) {
    let tick = park_after.map_or(REST_TICK, |limit| (limit / 4).min(REST_TICK));
    loop {
        tokio::time::sleep(tick).await;
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
    if let Some(pid) = child.and_then(|pid| i32::try_from(pid).ok()).and_then(rustix::process::Pid::from_raw) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::Term);
    }
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

async fn accept_clients(listener: UnixListener, queue: StdinQueue) {
    let mut clients = JoinSet::new();
    while let Ok((stream, _)) = listener.accept().await {
        while clients.try_join_next().is_some() {}
        clients.spawn(serve_client(stream, queue.clone()));
    }
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
    if !detached {
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

async fn feed_stdin(
    mut input: ChildStdin,
    mut priority: mpsc::Receiver<Frame>,
    mut lines: mpsc::Receiver<Frame>,
) -> ChildStdin {
    loop {
        let frame = tokio::select! {
            biased;
            Some(frame) = priority.recv() => frame,
            Some(frame) = lines.recv() => frame,
            else => break,
        };
        if write_frame(&mut input, frame).await.is_err() {
            break;
        }
    }
    input
}

async fn write_frame(input: &mut ChildStdin, Frame { mut line, reply }: Frame) -> io::Result<()> {
    if !line.ends_with('\n') {
        line.push('\n');
    }
    let write = async {
        input.write_all(line.as_bytes()).await?;
        input.flush().await
    };
    tokio::pin!(write);
    match tokio::time::timeout(STDIN_WRITE_TIMEOUT, &mut write).await {
        Ok(written) => written,
        Err(_) => {
            if let Some(reply) = reply {
                let _ = reply.send(STDIN_BLOCKED.into());
            }
            write.await
        }
    }
}

async fn copy_lines(mut source: impl AsyncRead + Unpin, mut file: File, activity: Activity, watcher: Watcher) -> io::Result<()> {
    let mut pending = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = source.read(&mut chunk).await?;
        if watcher.output_keeps_awake() {
            activity.touch();
        }
        if read == 0 {
            if !pending.is_empty() {
                pending.push(b'\n');
                file.write_all(&pending)?;
                watcher.lines(&pending);
            }
            return Ok(());
        }
        let start = pending.len();
        pending.extend_from_slice(&chunk[..read]);
        if let Some(position) = pending[start..].iter().rposition(|byte| *byte == b'\n') {
            let complete = start + position + 1;
            file.write_all(&pending[..complete])?;
            watcher.lines(&pending[..complete]);
            pending.drain(..complete);
        }
    }
}

async fn copy_rotating(mut source: impl AsyncRead + Unpin, path: PathBuf, limit: u64) -> io::Result<()> {
    let mut file = append(&path)?;
    let mut size = file.metadata()?.len();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = source.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        if size > 0 && size + read as u64 > limit {
            fs::rename(&path, path.with_extension("log.1"))?;
            file = append(&path)?;
            size = 0;
        }
        file.write_all(&chunk[..read])?;
        size += read as u64;
    }
}

fn append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod child {
    use super::*;
    use std::time::{Duration, Instant};

    fn config(root: &Path, script: &str) -> HostConfig {
        HostConfig {
            name: "lead".into(),
            generation: 1,
            state_dir: root.to_path_buf(),
            socket: None,
            idle_exit: None,
            provider: None,
            runtime: vec!["sh".into(), "-c".into(), script.into()],
        }
    }

    fn stream(root: &Path) -> String {
        fs::read_to_string(root.join("g1/stream.jsonl")).unwrap_or_default()
    }

    #[tokio::test]
    async fn stdout_lines_land_in_order_and_line_complete() {
        let root = tempfile::tempdir().unwrap();
        let host = SessionHost::spawn(&config(
            root.path(),
            r#"echo '{"n":1}'; printf '{"n":'; sleep 1; echo '2}'; echo '{"n":3}'"#,
        ))
        .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(stream(root.path()), "{\"n\":1}\n");
        assert_eq!(host.wait().await.unwrap(), 0);
        assert_eq!(stream(root.path()), "{\"n\":1}\n{\"n\":2}\n{\"n\":3}\n");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stderr_flood_does_not_stall_stdout() {
        let root = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let host = SessionHost::spawn(&config(
            root.path(),
            "head -c 67108864 /dev/zero >&2 & i=0; while [ $i -lt 2000 ]; do echo line$i; i=$((i+1)); done; wait",
        ))
        .unwrap();
        assert_eq!(host.wait().await.unwrap(), 0);
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
        let expected = (0..2000).map(|i| format!("line{i}\n")).collect::<String>();
        assert_eq!(stream(root.path()), expected);
        let current = fs::metadata(root.path().join("g1/stderr.log")).unwrap().len();
        let rotated = fs::metadata(root.path().join("g1/stderr.log.1")).unwrap().len();
        assert!(current <= STDERR_ROTATE_BYTES && rotated <= STDERR_ROTATE_BYTES);
    }

    #[tokio::test]
    async fn exit_codes_follow_the_child() {
        let root = tempfile::tempdir().unwrap();
        let exited = SessionHost::spawn(&config(root.path(), "exit 3")).unwrap();
        assert_eq!(exited.wait().await.unwrap(), 3);
        let killed = SessionHost::spawn(&config(root.path(), "kill -TERM $$")).unwrap();
        assert_eq!(killed.wait().await.unwrap(), 143);
    }

    #[tokio::test]
    async fn stdin_lines_reach_the_child() {
        let root = tempfile::tempdir().unwrap();
        let host = SessionHost::spawn(&config(root.path(), r#"read line; echo "got $line""#)).unwrap();
        host.stdin.enqueue(r#"{"type":"user"}"#.into(), None).await.unwrap();
        assert_eq!(host.wait().await.unwrap(), 0);
        assert_eq!(stream(root.path()), "got {\"type\":\"user\"}\n");
    }

    #[test]
    fn arguments_split_flags_from_runtime_argv() {
        let arguments = [
            "--name", "lead", "--generation", "2", "--state-dir", "/s", "--socket", "/r/l.sock", "--", "claude",
            "-p", "--name",
        ]
        .map(String::from);
        let config = HostConfig::parse(&arguments).unwrap();
        assert_eq!(config.name, "lead");
        assert_eq!(config.generation_dir(), PathBuf::from("/s/g2"));
        assert_eq!(config.socket_path().unwrap(), PathBuf::from("/r/l.sock"));
        assert_eq!(config.runtime, ["claude", "-p", "--name"]);
        for bad in [&arguments[..7], &arguments[2..], &[&arguments[..2], &arguments[..]].concat()] {
            assert!(matches!(HostConfig::parse(bad), Err(NorthError::Usage(_))), "{bad:?}");
        }
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
    async fn two_clients_interleave() {
        let root = tempfile::tempdir().unwrap();
        let (host, path) = host(root.path(), "exec head -n 4");
        let (mut a, mut b) = (Client::connect(&path).await, Client::connect(&path).await);
        let replies = [
            a.send(&user(1)).await,
            b.send(&user(2)).await,
            a.send(&user(3)).await,
            b.send(&user(4)).await,
        ];
        assert_eq!(replies, [1, 2, 3, 4].map(|n| format!(r#"{{"host_seq":{n}}}"#)));
        assert_eq!(host.wait().await.unwrap(), 0);
        assert_eq!(stream_lines(root.path(), 4).await, (1..=4).map(user).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn client_eof_only_detaches_that_client() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, path) = host(root.path(), r#"while read -r line; do echo "$line"; done"#);
        let pid = host.child.id();
        let mut leaving = Client::connect(&path).await;
        assert!(leaving.send(&user(1)).await.contains("host_seq"));
        leaving.write(&user(2)).await;
        drop(leaving);
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut staying = Client::connect(&path).await;
        assert!(staying.send(&user(3)).await.contains("host_seq"));
        assert_eq!(stream_lines(root.path(), 2).await, [user(1), user(3)]);
        assert_eq!(host.child.id(), pid);
        assert!(host.child.try_wait().unwrap().is_none());
        host.child.start_kill().unwrap();
        host.wait().await.unwrap();
    }

    #[tokio::test]
    async fn stale_partial_line_drops_only_that_client() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, path) = host(root.path(), r#"while read -r line; do echo "$line"; done"#);
        let (mut stale, mut live) = (Client::connect(&path).await, Client::connect(&path).await);
        stale.write(r#"{"type":"user""#).await;
        let started = Instant::now();
        assert!(live.send(&user(1)).await.contains("host_seq"));
        assert_eq!(tokio::time::timeout(Duration::from_secs(7), stale.reply()).await.unwrap(), None);
        assert!(started.elapsed() >= Duration::from_millis(4900), "{:?}", started.elapsed());
        assert!(live.send(&user(2)).await.contains("host_seq"));
        assert_eq!(stream_lines(root.path(), 2).await, [user(1), user(2)]);
        host.child.start_kill().unwrap();
        host.wait().await.unwrap();
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

    #[tokio::test]
    async fn stalled_stdin_reports_stdin_blocked() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, path) = host(root.path(), "exec sleep 60");
        let mut client = Client::connect(&path).await;
        let started = Instant::now();
        let frame = format!(r#"{{"type":"user","pad":"{}"}}"#, "x".repeat(512 * 1024));
        assert!(client.send(&frame).await.contains("host_seq"));
        let blocked = tokio::time::timeout(Duration::from_secs(6), client.reply()).await.unwrap();
        assert_eq!(blocked.as_deref(), Some(STDIN_BLOCKED));
        assert!(started.elapsed() < Duration::from_secs(6));
        host.child.start_kill().unwrap();
        host.wait().await.unwrap();
    }
}

#[cfg(test)]
mod fake {
    use super::*;
    use crate::claude_stream::{
        StreamFrame, decode_frame, encode_initialize, encode_interrupt, encode_user_message,
    };
    use std::sync::OnceLock;
    use std::time::Duration;

    const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

    fn fake_claude() -> String {
        static BUILT: OnceLock<String> = OnceLock::new();
        BUILT
            .get_or_init(|| {
                let exe = std::env::current_exe().unwrap();
                let profile = exe.parent().and_then(Path::parent).unwrap();
                let name = profile.file_name().and_then(|name| name.to_str()).unwrap();
                let status = std::process::Command::new(env!("CARGO"))
                    .args(["build", "--quiet", "--locked", "--example", "fake_claude", "--profile"])
                    .arg(if name == "debug" { "dev" } else { name })
                    .arg("--manifest-path")
                    .arg(std::env::current_dir().unwrap().join("Cargo.toml"))
                    .arg("--target-dir")
                    .arg(profile.parent().unwrap())
                    .status()
                    .unwrap();
                assert!(status.success());
                profile.join("examples/fake_claude").to_string_lossy().into_owned()
            })
            .clone()
    }

    async fn trace(name: &str, frames: &[String]) -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        let runtime = [fake_claude().as_str(), "-p", "--input-format", "stream-json", "--trace", name]
            .map(String::from)
            .to_vec();
        let host = SessionHost::spawn(&HostConfig {
            name: "lead".into(),
            generation: 1,
            state_dir: root.path().to_path_buf(),
            socket: None,
            idle_exit: None,
            provider: None,
            runtime,
        })
        .unwrap();
        for frame in frames {
            host.stdin.enqueue(frame.clone(), None).await.unwrap();
            let sent: serde_json::Value = serde_json::from_str(frame).unwrap();
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let text = fs::read_to_string(root.path().join("g1/stream.jsonl")).unwrap_or_default();
                    let observed = text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok());
                    if observed.into_iter().any(|seen| {
                        if sent["type"] == "control_request" {
                            seen["response"]["request_id"] == sent["request_id"]
                        } else if name == "idle-interrupt" {
                            seen["type"] == "result"
                        } else {
                            seen["type"] == "command_lifecycle" && seen["command_uuid"] == sent["uuid"]
                        }
                    }) {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            }).await.unwrap();
        }
        let code = tokio::time::timeout(Duration::from_secs(10), host.wait()).await.unwrap().unwrap();
        assert_eq!(code, 0);
        fs::read_to_string(root.path().join("g1/stream.jsonl"))
            .unwrap()
            .lines()
            .map(|line| signature(decode_frame(line).unwrap()))
            .collect()
    }

    fn signature(frame: StreamFrame) -> String {
        match frame {
            StreamFrame::Init(_) => "init".into(),
            StreamFrame::CommandLifecycle(c) => format!("{:?} {}", c.state, c.command_uuid),
            StreamFrame::User(u) if u.is_replay => format!("replay {}", u.uuid.unwrap_or_default()),
            StreamFrame::User(_) => "user".into(),
            StreamFrame::Assistant(a) => format!("assistant {}", a.user_message_uuids.join(",")),
            StreamFrame::Result(r) => format!(
                "result {} {}",
                r.terminal_reason.unwrap_or_default(),
                r.user_message_uuids.join(",")
            ),
            StreamFrame::TaskStarted(t) => format!("task_started {}", t.task_id),
            StreamFrame::TaskNotification(n) => format!("task {}", n.status.unwrap_or_default()),
            StreamFrame::ControlResponse(c) => format!("control {} {}", c.subtype, c.request_id),
            other => panic!("untyped frame {other:?}"),
        }
    }

    #[tokio::test]
    async fn happy_fold_completes_both_uuids_in_one_turn() {
        let frames = trace("fold", &[encode_user_message(A, "first"), encode_user_message(B, "second")]).await;
        assert_eq!(
            frames,
            [
                format!("Queued {A}"),
                format!("AcceptedIntoTurn {A}"),
                "init".into(),
                format!("replay {A}"),
                format!("assistant {A}"),
                "task_started fake-task".into(),
                format!("Queued {B}"),
                format!("AcceptedIntoTurn {B}"),
                format!("replay {B}"),
                "task completed".into(),
                format!("assistant {B}"),
                format!("result completed {A},{B}"),
                format!("Completed {A}"),
                format!("Completed {B}"),
            ]
        );
    }

    #[tokio::test]
    async fn interrupt_after_completion_hits_an_idle_session() {
        let frames = trace("idle-interrupt", &[encode_user_message(A, "first"), encode_interrupt("int1")]).await;
        assert_eq!(
            frames,
            [
                format!("Queued {A}"),
                format!("AcceptedIntoTurn {A}"),
                "init".into(),
                format!("replay {A}"),
                format!("assistant {A}"),
                format!("result completed {A}"),
                format!("Completed {A}"),
                "control success int1".into(),
            ]
        );
    }

    #[tokio::test]
    async fn interrupt_cancels_the_queued_input() {
        let frames = trace(
            "cancel-queued",
            &[encode_user_message(A, "first"), encode_user_message(B, "second"), encode_interrupt("int1")],
        )
        .await;
        assert_eq!(
            frames,
            [
                format!("Queued {A}"),
                format!("AcceptedIntoTurn {A}"),
                "init".into(),
                format!("replay {A}"),
                format!("assistant {A}"),
                "task_started fake-task".into(),
                format!("Queued {B}"),
                "control success int1".into(),
                "task stopped".into(),
                "user".into(),
                "user".into(),
                format!("result aborted_tools {A}"),
                format!("Cancelled {A}"),
                format!("Cancelled {B}"),
            ]
        );
    }

    #[tokio::test]
    async fn initialize_is_answered_with_its_request_id() {
        let frames = trace("idle-interrupt", &[encode_initialize("init1"), encode_interrupt("int1")]).await;
        assert_eq!(frames, ["control success init1", "control success int1"]);
    }
}
