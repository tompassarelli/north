//! `north capacity offload take`: this host polls the hosts it reaches for a queued agent heavy run, takes the oldest when it has free slots beyond what interactive work keeps, and runs it under its own heavy admission. The polled host opens nothing outward: the lane streams in and the output streams back over the ssh connection this host opened.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::remote::{SYNCED, script, words};
use crate::capacity::admission::offload_room;
use crate::capacity::handover::{Frame, Offer};
use crate::land_offload::room::{self, Room};
use crate::land_offload::{build_hosts, ship::quote};

const WAIT_SECONDS: u64 = 3;
const UNREACHABLE_BACKOFF: Duration = Duration::from_secs(10);
const REFUSED_PAUSE: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_secs(5);
const CHUNK: usize = 32 * 1024;
const NOTHING_QUEUED: i32 = 3;
const USAGE: &str = "usage: north capacity offload take [--once]";
const RELEASE: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host hand-over --wait-seconds"#;

/// Why this host takes no queued run now: fewer free slots than a heavy run needs once the interactive reserve is kept (the admission limit already excludes it) and the runs it took that have not yet started are counted, a land check waiting, CPU pressure, memory or disk; None when it can take one.
pub(super) fn refusal(room: Option<&Room>, starting: usize) -> Option<String> {
    let Some(room) = room else { return Some("this host's room is unreadable".to_owned()) };
    room::build_shortfall_reserving(room, starting as f64 * offload_room().0)
}

enum Polled {
    Idle,
    Unreachable(String),
    Offered(Box<Child>, BufReader<ChildStdout>, Offer),
}

fn poll(ssh: &str) -> Polled {
    let remote = format!("sh -c {}", quote(&format!("{RELEASE} {WAIT_SECONDS}")));
    let spawned = Command::new("ssh")
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", ssh, "--", &remote])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return Polled::Unreachable(format!("ssh: {error}")),
    };
    let Some(stdout) = child.stdout.take() else { return Polled::Unreachable("ssh gave no pipes".to_owned()) };
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let read = reader.read_line(&mut line).unwrap_or(0);
    if read > 0 && let Some(offer) = Offer::from_line(&line) {
        return Polled::Offered(Box::new(child), reader, offer);
    }
    let status = child.wait().ok().and_then(|status| status.code());
    match status {
        Some(NOTHING_QUEUED) | Some(0) => Polled::Idle,
        other => Polled::Unreachable(format!("ssh {ssh} ended with {}", other.map_or_else(|| "a signal".to_owned(), |code| code.to_string()))),
    }
}

pub(super) struct Sender(Mutex<Box<dyn Write + Send>>);

impl Sender {
    pub(super) fn to(sink: impl Write + Send + 'static) -> Self {
        Self(Mutex::new(Box::new(sink)))
    }

    pub(super) fn send(&self, frame: &Frame) {
        if let Ok(mut sink) = self.0.lock() {
            let _ = frame.write(&mut *sink);
        }
    }

    fn pipe(&self, mut source: impl Read, frame: fn(Vec<u8>) -> Frame) {
        let mut chunk = vec![0u8; CHUNK];
        while let Ok(read) = source.read(&mut chunk) {
            if read == 0 {
                break;
            }
            self.send(&frame(chunk[..read].to_vec()));
        }
    }
}

/// The script's stdin, which stays open for the whole run because its watcher reads the end of it as the order to stop; closed once the run ends, since the watcher's `cat` keeps the script's stderr open until then.
#[derive(Clone)]
struct Lane(Arc<Mutex<Option<ChildStdin>>>);

impl Lane {
    fn close(&self) {
        if let Ok(mut stdin) = self.0.lock() {
            drop(stdin.take());
        }
    }
}

impl Write for Lane {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self.0.lock().map_err(|_| std::io::ErrorKind::BrokenPipe)?.as_mut() {
            Some(stdin) => stdin.write(bytes),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.0.lock().map_err(|_| std::io::ErrorKind::BrokenPipe)?.as_mut() {
            Some(stdin) => stdin.flush(),
            None => Ok(()),
        }
    }
}

/// Counts a taken run as starting until its command has begun and its lease shows in this host's own admission.
pub(super) struct Starting {
    pub(super) count: Arc<AtomicUsize>,
    pub(super) released: AtomicBool,
}

impl Starting {
    fn release(&self) {
        if !self.released.swap(true, Ordering::SeqCst) {
            self.count.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn log(line: &str) {
    println!("taker: {line}");
    let _ = std::io::stdout().flush();
}

fn exit_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).and_then(|code| u8::try_from(code).ok()).unwrap_or(255)
}

fn describe(offer: &Offer) -> String {
    format!("{} ({} {}: {})", offer.owner, offer.repo, &offer.head[..offer.head.len().min(12)], offer.command.join(" "))
}

fn hang_up(mut ssh: Child) {
    let _ = ssh.kill();
    let _ = ssh.wait();
}

/// Rebuilds the lane here and runs the offer under this host's own heavy admission, relaying its output as frames; Some((exit code, whether the lane synced)) once the script ends, None when it could not start.
pub(super) fn run_lane(offer: &Offer, host: &str, reader: impl Read + Send + 'static, sender: &Arc<Sender>, starting: &Arc<Starting>, environment: &dyn Fn(&mut Command)) -> Option<(u8, bool)> {
    let mut command = Command::new("sh");
    command.arg("-c").arg(script()).arg("offload").args(words(offer)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    environment(&mut command);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            sender.send(&Frame::NotSynced(format!("sh: {error}")));
            return None;
        }
    };
    let (output, errors) = (child.stdout.take()?, child.stderr.take()?);
    let lane = Lane(Arc::new(Mutex::new(child.stdin.take())));
    let payload = offer.bundle_bytes + offer.patch_bytes;
    let feeder = {
        let mut lane = lane.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let _ = std::io::copy(&mut (&mut reader).take(payload), &mut lane);
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
            lane.close();
        })
    };
    let synced = Arc::new(AtomicBool::new(false));
    let relay_output = {
        let (sender, synced, starting, host) = (Arc::clone(sender), Arc::clone(&synced), Arc::clone(starting), host.to_owned());
        std::thread::spawn(move || relay_stdout(output, &host, &sender, &synced, starting))
    };
    let relay_errors = {
        let sender = Arc::clone(sender);
        std::thread::spawn(move || sender.pipe(errors, Frame::Err))
    };
    let status = child.wait().map(exit_code).unwrap_or(255);
    lane.close();
    let _ = relay_output.join();
    let _ = relay_errors.join();
    drop(feeder);
    Some((status, synced.load(Ordering::SeqCst)))
}

fn relay_stdout(output: impl Read, host: &str, sender: &Sender, synced: &AtomicBool, starting: Arc<Starting>) {
    let mut output = BufReader::new(output);
    let mut first = String::new();
    if output.read_line(&mut first).is_ok_and(|read| read > 0) && first.trim_end() == SYNCED {
        synced.store(true, Ordering::SeqCst);
        sender.send(&Frame::Started(host.to_owned()));
        std::thread::spawn(move || {
            std::thread::sleep(SETTLE);
            starting.release();
        });
    } else if !first.is_empty() {
        sender.send(&Frame::Out(first.into_bytes()));
    }
    sender.pipe(output, Frame::Out);
}

fn serve(source: &str, host: &str, mut ssh: Child, reader: BufReader<ChildStdout>, offer: Offer, starting: Arc<Starting>) {
    let Some(stdin) = ssh.stdin.take() else { return starting.release() };
    let sender = Arc::new(Sender::to(stdin));
    if let Some(why) = offer.refusal() {
        log(&format!("refused a run from {source}: {why}"));
        sender.send(&Frame::NotSynced(why));
    } else {
        log(&format!("took {} from {source}", describe(&offer)));
        match run_lane(&offer, host, reader, &sender, &starting, &|_| {}) {
            Some((status, true)) => {
                sender.send(&Frame::Exit(status));
                log(&format!("finished {} from {source}: exit {status}", offer.owner));
            }
            Some((status, false)) => {
                sender.send(&Frame::NotSynced(format!("the lane did not sync here: exit {status}")));
                log(&format!("could not sync {} from {source}: exit {status}", offer.owner));
            }
            None => {}
        }
    }
    hang_up(ssh);
    starting.release();
}

pub fn run(arguments: &[String]) -> u8 {
    let once = arguments == ["--once"];
    if !arguments.is_empty() && !once {
        eprintln!("{USAGE}");
        return 2;
    }
    let sources = build_hosts();
    if sources.is_empty() {
        eprintln!("north capacity offload take: this host declares no reaches to a host that offloads builds");
        return 2;
    }
    let host = crate::platform::host::name();
    let count = Arc::new(AtomicUsize::new(0));
    let mut printed: Option<String> = None;
    let mut taken = false;
    loop {
        let mut backoff = false;
        let mut polled = false;
        for (name, ssh) in &sources {
            let reason = refusal(room::read_local().as_ref(), count.load(Ordering::SeqCst));
            if let Some(why) = reason {
                if printed.as_deref() != Some(why.as_str()) {
                    log(&format!("not taking from {name}: {why}"));
                    printed = Some(why);
                }
                continue;
            }
            printed = None;
            polled = true;
            match poll(ssh) {
                Polled::Idle => {}
                Polled::Unreachable(why) => {
                    log(&format!("{name} unreachable: {why}"));
                    backoff = true;
                }
                Polled::Offered(child, reader, offer) => {
                    taken = true;
                    count.fetch_add(1, Ordering::SeqCst);
                    let starting = Arc::new(Starting { count: Arc::clone(&count), released: AtomicBool::new(false) });
                    let (source, host) = (name.clone(), host.clone());
                    let handle = std::thread::spawn(move || serve(&source, &host, *child, reader, offer, starting));
                    if once {
                        let _ = handle.join();
                    }
                }
            }
        }
        if once {
            return if taken { 0 } else { 1 };
        }
        if backoff {
            std::thread::sleep(UNREACHABLE_BACKOFF);
        } else if !polled {
            std::thread::sleep(REFUSED_PAUSE);
        }
    }
}
