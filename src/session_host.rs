use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::error::{NorthError, NorthResult};

pub const STDERR_ROTATE_BYTES: u64 = 10 * 1024 * 1024;
pub const STDIN_LINES: usize = 64;

const USAGE: &str =
    "Usage: north session-host --name NAME --generation N --state-dir DIR -- RUNTIME [ARG...]";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostConfig {
    pub name: String,
    pub generation: u64,
    pub state_dir: PathBuf,
    pub runtime: Vec<String>,
}

impl HostConfig {
    pub fn parse(arguments: &[String]) -> NorthResult<Self> {
        let usage = || NorthError::Usage(USAGE.into());
        let (mut name, mut generation, mut state_dir) = (None, None, None);
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
                runtime,
            }),
            _ => Err(usage()),
        }
    }

    pub fn generation_dir(&self) -> PathBuf {
        self.state_dir.join(format!("g{}", self.generation))
    }
}

pub struct SessionHost {
    pub stdin: mpsc::Sender<String>,
    child: Child,
    writer: JoinHandle<ChildStdin>,
    stdout: JoinHandle<io::Result<()>>,
    stderr: JoinHandle<io::Result<()>>,
}

impl SessionHost {
    pub fn spawn(config: &HostConfig) -> NorthResult<Self> {
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
        let (stdin, lines) = mpsc::channel(STDIN_LINES);
        Ok(Self {
            stdin,
            child,
            writer: tokio::spawn(feed_stdin(input, lines)),
            stdout: tokio::spawn(copy_lines(output, stream)),
            stderr: tokio::spawn(copy_rotating(errors, log, STDERR_ROTATE_BYTES)),
        })
    }

    pub async fn wait(self) -> NorthResult<u8> {
        let Self { stdin, mut child, writer, stdout, stderr } = self;
        drop(stdin);
        let status = child.wait().await?;
        let drained = (stdout.await, stderr.await);
        writer.abort();
        let _ = writer.await;
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

pub async fn run(arguments: &[String]) -> NorthResult<u8> {
    SessionHost::spawn(&HostConfig::parse(arguments)?)?.wait().await
}

async fn feed_stdin(mut input: ChildStdin, mut lines: mpsc::Receiver<String>) -> ChildStdin {
    while let Some(mut line) = lines.recv().await {
        if !line.ends_with('\n') {
            line.push('\n');
        }
        if input.write_all(line.as_bytes()).await.is_err() || input.flush().await.is_err() {
            break;
        }
    }
    input
}

async fn copy_lines(mut source: impl AsyncRead + Unpin, mut file: File) -> io::Result<()> {
    let mut pending = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = source.read(&mut chunk).await?;
        if read == 0 {
            if !pending.is_empty() {
                pending.push(b'\n');
                file.write_all(&pending)?;
            }
            return Ok(());
        }
        let start = pending.len();
        pending.extend_from_slice(&chunk[..read]);
        if let Some(position) = pending[start..].iter().rposition(|byte| *byte == b'\n') {
            let complete = start + position + 1;
            file.write_all(&pending[..complete])?;
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
        host.stdin.send(r#"{"type":"user"}"#.into()).await.unwrap();
        assert_eq!(host.wait().await.unwrap(), 0);
        assert_eq!(stream(root.path()), "got {\"type\":\"user\"}\n");
    }

    #[test]
    fn arguments_split_flags_from_runtime_argv() {
        let arguments = ["--name", "lead", "--generation", "2", "--state-dir", "/s", "--", "claude", "-p", "--name"]
            .map(String::from);
        let config = HostConfig::parse(&arguments).unwrap();
        assert_eq!(config.name, "lead");
        assert_eq!(config.generation_dir(), PathBuf::from("/s/g2"));
        assert_eq!(config.runtime, ["claude", "-p", "--name"]);
        for bad in [&arguments[..7], &arguments[2..], &[&arguments[..2], &arguments[..]].concat()] {
            assert!(matches!(HostConfig::parse(bad), Err(NorthError::Usage(_))), "{bad:?}");
        }
    }
}
