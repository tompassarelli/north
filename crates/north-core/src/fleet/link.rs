//! The session host's child for a session placed on another machine. The CLI runs there in a user
//! service that outlives any one ssh connection (see `remote`), reading an input file and appending
//! to an output file. Each connection reports how much input has arrived and resumes the output
//! where this side stopped, so a stalled or dropped link loses neither frames nor the CLI's
//! background work: the link reconnects until the CLI ends, this side's input closes or the host
//! stops it, and only the last two stop the service.
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::error::{NorthError, NorthResult};

use super::runtime::binary;

const USAGE: &str = "Usage: north session-link --ssh DESTINATION --unit UNIT --dir DIR --nonce NONCE --launch COMMAND";
const OPTIONS: [&str; 11] = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4", "-o", "ControlPath=none"];
const TRANSPORT_FAILED: i32 = 255;
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MOST: Duration = Duration::from_secs(15);
const DRAIN: Duration = Duration::from_secs(2);
const STOP_WAIT: Duration = Duration::from_secs(4);
const STOPPED: u8 = 143;

/// Run on the remote machine as `sh -c ATTACH DIR UNIT NONCE OFFSET` for every connection: it ends
/// any earlier connection's relay, prints `INPUT_BYTES MAIN_PID OUTPUT_BYTES` (or `gone` when that
/// directory holds another launch), then appends this connection's input and streams the output
/// from OFFSET until the service's main process exits, and exits with the CLI's status.
const ATTACH: &str = r#"cd "$0" && [ "$(cat nonce 2>/dev/null)" = "$2" ] || { echo gone; exit 0; }
if read -r p b 2>/dev/null < attach && [ "$(cut -d' ' -f22 "/proc/$p/stat" 2>/dev/null)" = "$b" ]; then
  kill -s KILL -- "-$p" 2>/dev/null; n=0
  while kill -s 0 -- "-$p" 2>/dev/null && [ $n -lt 50 ]; do sleep 0.1; n=$((n + 1)); done
fi
echo "$$ $(cut -d' ' -f22 /proc/$$/stat)" > attach
cli=$(systemctl --user show -p MainPID --value "$1"); cli=${cli:-0}
echo "$(stat -c %s in) $cli $(stat -c %s out)"
if [ "$cli" = 0 ]; then tail -c +$(($3 + 1)) out; exit "$(cat status 2>/dev/null || echo 143)"; fi
exec 3<&0
cat <&3 >> in & feed=$!
tail -c +$(($(stat -c %s err) + 1)) -f --pid="$cli" err >&2 &
tail -c +$(($3 + 1)) -f --pid="$cli" out
kill "$feed" 2>/dev/null
exit "$(cat status 2>/dev/null || echo 143)""#;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Link {
    pub ssh: PathBuf,
    pub destination: String,
    pub unit: String,
    pub dir: String,
    pub nonce: String,
    pub launch: String,
}

#[derive(Debug, Eq, PartialEq)]
enum Connection {
    Ended(u8),
    Lost { attached: bool },
    Gone,
    Closed,
    Stopped,
}

#[derive(Default)]
struct Unsent {
    base: u64,
    bytes: Vec<u8>,
}

impl Unsent {
    fn after(&mut self, delivered: u64) -> Vec<u8> {
        let skip = usize::try_from(delivered.saturating_sub(self.base)).unwrap_or(usize::MAX).min(self.bytes.len());
        self.bytes.drain(..skip);
        self.base += skip as u64;
        self.bytes.clone()
    }
}

struct Header {
    cli: u64,
    output: u64,
}

struct Stream<'a, O> {
    unsent: Unsent,
    received: u64,
    input: mpsc::UnboundedReceiver<Vec<u8>>,
    output: &'a mut O,
}

impl<O: AsyncWrite + Unpin> Stream<'_, O> {
    async fn emit(&mut self, bytes: &[u8]) -> bool {
        let written = self.output.write_all(bytes).await.and(self.output.flush().await).is_ok();
        self.received += bytes.len() as u64;
        written
    }
}

impl Link {
    pub fn parse(arguments: &[String]) -> NorthResult<Self> {
        let usage = || NorthError::Usage(USAGE.into());
        let (mut destination, mut unit, mut dir, mut nonce, mut launch) = (None, None, None, None, None);
        for pair in arguments.chunks(2) {
            let [flag, value] = pair else { return Err(usage()) };
            let slot = match flag.as_str() {
                "--ssh" => &mut destination,
                "--unit" => &mut unit,
                "--dir" => &mut dir,
                "--nonce" => &mut nonce,
                "--launch" => &mut launch,
                _ => return Err(usage()),
            };
            if value.is_empty() || slot.replace(value.clone()).is_some() {
                return Err(usage());
            }
        }
        match (destination, unit, dir, nonce, launch) {
            (Some(destination), Some(unit), Some(dir), Some(nonce), Some(launch)) => Ok(Self { ssh: binary("ssh"), destination, unit, dir, nonce, launch }),
            _ => Err(usage()),
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        [("--ssh", &self.destination), ("--unit", &self.unit), ("--dir", &self.dir), ("--nonce", &self.nonce), ("--launch", &self.launch)]
            .into_iter()
            .flat_map(|(flag, value)| [flag.to_owned(), value.clone()])
            .collect()
    }

    pub async fn relay(&self, input: impl AsyncRead + Unpin + Send + 'static, output: &mut (impl AsyncWrite + Unpin), stop: impl Future<Output = ()>) -> NorthResult<u8> {
        let (sender, receiver) = mpsc::unbounded_channel();
        tokio::spawn(read_chunks(input, sender));
        let mut stream = Stream { unsent: Unsent::default(), received: 0, input: receiver, output };
        tokio::pin!(stop);
        let (mut launched, mut retry) = (false, RETRY_FIRST);
        loop {
            let ended = match self.connect(launched, &mut stream, stop.as_mut()).await? {
                Connection::Ended(code) => return Ok(code),
                Connection::Gone => {
                    eprintln!("north session-link: {} no longer holds this session's CLI", self.destination);
                    return Ok(1);
                }
                Connection::Closed => Some(0),
                Connection::Stopped => Some(STOPPED),
                Connection::Lost { attached } => {
                    launched |= attached;
                    retry = if attached { RETRY_FIRST } else { (retry * 2).min(RETRY_MOST) };
                    eprintln!("north session-link: the link to {} dropped; reconnecting", self.destination);
                    self.pause(retry, &mut stream, stop.as_mut()).await
                }
            };
            if let Some(code) = ended {
                self.stop_remote().await;
                return Ok(code);
            }
        }
    }

    async fn pause<O>(&self, retry: Duration, stream: &mut Stream<'_, O>, mut stop: Pin<&mut impl Future<Output = ()>>) -> Option<u8> {
        let resume = tokio::time::sleep(retry);
        tokio::pin!(resume);
        loop {
            tokio::select! {
                () = &mut resume => return None,
                () = &mut stop => return Some(STOPPED),
                chunk = stream.input.recv() => match chunk {
                    Some(bytes) => stream.unsent.bytes.extend_from_slice(&bytes),
                    None => return Some(0),
                },
            }
        }
    }

    async fn connect<O: AsyncWrite + Unpin>(&self, launched: bool, stream: &mut Stream<'_, O>, mut stop: Pin<&mut impl Future<Output = ()>>) -> NorthResult<Connection> {
        let attach = self.attach(if launched { stream.received } else { 0 });
        let command = if launched { attach } else { format!("{}; {attach}", self.launch) };
        let mut child = self.ssh(&command).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
        let missing = || NorthError::State("ssh pipe was not captured".into());
        let to_remote = forward(child.stdin.take().ok_or_else(missing)?);
        let (sender, mut from_remote) = mpsc::unbounded_channel();
        tokio::spawn(read_chunks(child.stdout.take().ok_or_else(missing)?, sender));
        let (mut head, mut header) = (Vec::new(), None::<Header>);
        let (mut status, mut drained_by) = (None, None::<Instant>);
        loop {
            let drain = async {
                match drained_by {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                () = &mut stop => return Ok(Connection::Stopped),
                chunk = from_remote.recv() => {
                    let Some(mut bytes) = chunk else { break };
                    if header.is_none() {
                        head.extend_from_slice(&bytes);
                        let Some(end) = head.iter().position(|byte| *byte == b'\n') else { continue };
                        bytes = head.split_off(end + 1);
                        let Some((delivered, parsed)) = parse_header(&String::from_utf8_lossy(&head[..end])) else { return Ok(Connection::Gone) };
                        let _ = to_remote.send(stream.unsent.after(delivered));
                        header = Some(parsed);
                    }
                    if !stream.emit(&bytes).await {
                        return Ok(Connection::Closed);
                    }
                }
                chunk = stream.input.recv() => {
                    let Some(bytes) = chunk else { return Ok(Connection::Closed) };
                    stream.unsent.bytes.extend_from_slice(&bytes);
                    if header.is_some() {
                        let _ = to_remote.send(bytes);
                    }
                }
                exited = child.wait(), if status.is_none() => {
                    status = Some(exited?);
                    drained_by = Some(Instant::now() + DRAIN);
                }
                () = drain => break,
            }
        }
        let code = match status {
            Some(status) => status.code(),
            None => tokio::time::timeout(DRAIN, child.wait()).await.ok().transpose()?.and_then(|status| status.code()),
        };
        Ok(match (header, code) {
            (None, _) => Connection::Lost { attached: false },
            (Some(header), Some(code)) if code != TRANSPORT_FAILED && header.cli != 0 => Connection::Ended(u8::try_from(code).unwrap_or(1)),
            (Some(header), Some(code)) if header.cli == 0 && stream.received >= header.output => Connection::Ended(u8::try_from(code).unwrap_or(1)),
            (Some(_), _) => Connection::Lost { attached: true },
        })
    }

    fn attach(&self, offset: u64) -> String {
        format!("exec sh -c {} {} {} {} {offset}", quote(ATTACH), quote(&self.dir), quote(&self.unit), quote(&self.nonce))
    }

    fn ssh(&self, command: &str) -> Command {
        let mut ssh = Command::new(&self.ssh);
        ssh.args(OPTIONS).arg(&self.destination).arg("--").arg(command).stderr(Stdio::inherit()).kill_on_drop(true);
        ssh
    }

    async fn stop_remote(&self) {
        let stop = format!("systemctl --user stop --no-block {}", quote(&self.unit));
        let Ok(mut child) = self.ssh(&stop).stdin(Stdio::null()).stdout(Stdio::null()).spawn() else { return };
        if tokio::time::timeout(STOP_WAIT, child.wait()).await.is_err() {
            eprintln!("north session-link: could not reach {} to stop {}; its detached watchdog stops it", self.destination, self.unit);
        }
    }
}

pub async fn run(arguments: &[String]) -> NorthResult<u8> {
    let link = Link::parse(arguments)?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let stop = async move {
        term.recv().await;
    };
    link.relay(tokio::io::stdin(), &mut tokio::io::stdout(), stop).await
}

fn parse_header(line: &str) -> Option<(u64, Header)> {
    let mut words = line.split_whitespace().map(str::parse::<u64>);
    match (words.next(), words.next(), words.next(), words.next()) {
        (Some(Ok(delivered)), Some(Ok(cli)), Some(Ok(output)), None) => Some((delivered, Header { cli, output })),
        _ => None,
    }
}

fn forward(mut stdin: ChildStdin) -> mpsc::UnboundedSender<Vec<u8>> {
    let (sender, mut writes) = mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(async move {
        while let Some(bytes) = writes.recv().await {
            if stdin.write_all(&bytes).await.and(stdin.flush().await).is_err() {
                break;
            }
        }
    });
    sender
}

async fn read_chunks(mut source: impl AsyncRead + Unpin, sink: mpsc::UnboundedSender<Vec<u8>>) {
    let mut chunk = vec![0; 64 * 1024];
    while let Ok(read) = source.read(&mut chunk).await {
        if read == 0 || sink.send(chunk[..read].to_vec()).is_err() {
            break;
        }
    }
}

pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
