//! `north capacity offload take`: this host polls the hosts it reaches for a queued agent heavy run, takes the oldest when this host or another build host it reaches has free slots beyond what interactive work keeps, and runs it on the one with the most free cores under that host's own heavy admission. The polled host opens nothing outward: the lane streams in and the output streams back over the ssh connection this host opened.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::remote::{SYNCED, script, words};
use crate::capacity::admission::offload_room;
use crate::capacity::handover::{Frame, Offer};
use crate::land_offload::room::{self, Room};
use crate::land_offload::{build_hosts, ship::quote};

const WAIT_SECONDS: u64 = 3;
const UNREACHABLE_BACKOFF: Duration = Duration::from_secs(10);
const REFUSED_PAUSE: Duration = Duration::from_secs(2);
const SETTLE: Duration = Duration::from_secs(5);
const ROOM_FRESH: Duration = Duration::from_secs(10);
const ROOM_READ_LIMIT: Duration = Duration::from_secs(8);
const CHUNK: usize = 32 * 1024;
const NOTHING_QUEUED: i32 = 3;
const USAGE: &str = "usage: north capacity offload take [--once]";
const RELEASE: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host hand-over --wait-seconds"#;

/// Why a target takes no pulled run now: fewer free slots than a heavy run needs once its interactive reserve is kept (its admission limit already excludes it) and the runs sent to it that have not yet started are counted, a land check waiting, CPU pressure, memory or disk; None when it can take one.
pub(super) fn refusal(room: Option<&Room>, starting: usize) -> Option<String> {
    let Some(room) = room else { return Some("its room is unreadable".to_owned()) };
    room::build_shortfall_reserving(room, starting as f64 * offload_room().0)
}

/// A host a pulled run may go to, as the taker sees it now: this host itself (no ssh) or a build host it reaches.
pub(super) struct Target<'a> {
    pub(super) name: &'a str,
    pub(super) ssh: Option<&'a str>,
    pub(super) room: Option<&'a Room>,
    pub(super) starting: usize,
}

/// The target with the most free cores among those with room for a heavy run, or every target's reason; nothing is taken while this host holds its game reserve, since a pulled run streams through it wherever it runs.
pub(super) fn pick<'a>(targets: &[Target<'a>], game_reserved: bool) -> Result<(&'a str, Option<&'a str>), Vec<String>> {
    if game_reserved {
        return Err(vec!["this host holds its game reserve".to_owned()]);
    }
    let free = |target: &Target| target.room.map_or(f64::NEG_INFINITY, |room| room.admission.free_cpus() - target.starting as f64 * offload_room().0);
    let open = targets.iter().filter(|target| refusal(target.room, target.starting).is_none());
    match open.max_by(|left, right| free(left).total_cmp(&free(right))) {
        Some(target) => Ok((target.name, target.ssh)),
        None => Err(targets.iter().filter_map(|target| refusal(target.room, target.starting).map(|why| format!("{}: {why}", target.name))).collect()),
    }
}

enum Polled {
    Idle,
    Unreachable(String),
    Offered(Box<Child>, BufReader<ChildStdout>, Box<Offer>),
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
        return Polled::Offered(Box::new(child), reader, Box::new(offer));
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

/// Rebuilds the lane on `host` (this host, or the build host `ssh` names) and runs the offer under that host's own heavy admission, relaying its output as frames; Some((exit code, whether the lane synced)) once the script ends, None when it could not start.
pub(super) fn run_lane(offer: &Offer, host: &str, ssh: Option<&str>, reader: impl Read + Send + 'static, sender: &Arc<Sender>, starting: &Arc<Starting>, environment: &dyn Fn(&mut Command)) -> Option<(u8, bool)> {
    let mut command = transport(offer, ssh);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
    let stdin = child.stdin.take()?;
    let payload = offer.bundle_bytes + offer.patch_bytes;
    let feeder = std::thread::spawn(move || {
        let (mut reader, mut stdin) = (reader, stdin);
        let _ = std::io::copy(&mut (&mut reader).take(payload), &mut stdin);
        (reader, stdin)
    });
    let synced = Arc::new(AtomicBool::new(false));
    let relay_output = {
        let (sender, synced, starting, host) = (Arc::clone(sender), Arc::clone(&synced), Arc::clone(starting), host.to_owned());
        std::thread::spawn(move || relay_stdout(output, &host, &sender, &synced, starting))
    };
    let relay_errors = {
        let sender = Arc::clone(sender);
        std::thread::spawn(move || sender.pipe(errors, Frame::Err))
    };
    // the script's watcher reads the end of its stdin as the order to stop, and its `cat` holds stderr open until then
    let held = feeder.join().ok().map(|(mut reader, stdin)| {
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        stdin
    });
    let status = child.wait().map(exit_code).unwrap_or(255);
    drop(held);
    let _ = relay_output.join();
    let _ = relay_errors.join();
    Some((status, synced.load(Ordering::SeqCst)))
}

/// The lane script and its words, run here or on the build host `ssh` names.
fn transport(offer: &Offer, ssh: Option<&str>) -> Command {
    match ssh {
        None => {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script()).arg("offload").args(words(offer));
            command
        }
        Some(ssh) => {
            let quoted: Vec<String> = words(offer).iter().map(|word| quote(word)).collect();
            let mut command = Command::new("ssh");
            command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", ssh, "--", &format!("sh -c {} offload {}", quote(&script()), quoted.join(" "))]);
            command
        }
    }
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

/// Where a taken run came from and where it runs.
struct Route {
    source: String,
    target: String,
    ssh: Option<String>,
}

fn serve(route: &Route, mut ssh: Child, reader: BufReader<ChildStdout>, offer: Offer, starting: Arc<Starting>) {
    let Some(stdin) = ssh.stdin.take() else { return starting.release() };
    let sender = Arc::new(Sender::to(stdin));
    let source = route.source.as_str();
    if let Some(why) = offer.refusal() {
        log(&format!("refused a run from {source}: {why}"));
        sender.send(&Frame::NotSynced(why));
    } else {
        log(&format!("PULLED host={} owner={} from={source}: {}", route.target, offer.owner, describe(&offer)));
        match run_lane(&offer, &route.target, route.ssh.as_deref(), reader, &sender, &starting, &|_| {}) {
            Some((status, true)) => {
                sender.send(&Frame::Exit(status));
                log(&format!("finished {} from {source} on {}: exit {status}", offer.owner, route.target));
            }
            Some((status, false)) => {
                sender.send(&Frame::NotSynced(format!("the lane did not sync on {}: exit {status}", route.target)));
                log(&format!("could not sync {} from {source} on {}: exit {status}", offer.owner, route.target));
            }
            None => {}
        }
    }
    hang_up(ssh);
    starting.release();
}

/// Whether this host holds its game reserve, during which it takes nothing.
fn game_reserved() -> bool {
    crate::capacity::mode::confined_agent_cpus(&crate::capacity::agent_load::Host::from_env()).is_some()
}

/// The build hosts' rooms, read over ssh at most every `ROOM_FRESH`, since each read is a round trip.
struct RemoteRooms(HashMap<String, (Instant, Option<Room>)>);

impl RemoteRooms {
    fn refresh(&mut self, hosts: &[(String, String)]) {
        for (name, ssh) in hosts {
            if self.0.get(name).is_none_or(|(read, _)| read.elapsed() >= ROOM_FRESH) {
                self.0.insert(name.clone(), (Instant::now(), room::read_within(ssh, ROOM_READ_LIMIT)));
            }
        }
    }

    fn room(&self, name: &str) -> Option<&Room> {
        self.0.get(name).and_then(|(_, room)| room.as_ref())
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let once = arguments == ["--once"];
    if !arguments.is_empty() && !once {
        eprintln!("{USAGE}");
        return 2;
    }
    let hosts = build_hosts();
    if hosts.is_empty() {
        eprintln!("north capacity offload take: this host declares no reaches to a host that offloads builds");
        return 2;
    }
    if let Ok(binary) = std::env::current_exe() {
        log(&format!("running {}", binary.display()));
    }
    let here = crate::platform::host::name();
    let starting: HashMap<String, Arc<AtomicUsize>> = std::iter::once(here.clone()).chain(hosts.iter().map(|(name, _)| name.clone())).map(|name| (name, Arc::new(AtomicUsize::new(0)))).collect();
    let serving = Arc::new(AtomicUsize::new(0));
    let mut remote_rooms = RemoteRooms(HashMap::new());
    let mut printed: HashMap<String, String> = HashMap::new();
    let mut taken = false;
    loop {
        if !once && serving.load(Ordering::SeqCst) == 0 && north_base::platform::north_binary::superseded() {
            log("the promoted runtime moved; exiting so the service starts it");
            return 0;
        }
        let reserved = game_reserved();
        let here_room = if reserved { None } else { room::read_local() };
        if !reserved {
            remote_rooms.refresh(&hosts);
        }
        let mut backoff = false;
        let mut polled = false;
        for (source, source_ssh) in &hosts {
            let count = |name: &str| starting.get(name).map_or(0, |count| count.load(Ordering::SeqCst));
            let mut targets = vec![Target { name: &here, ssh: None, room: here_room.as_ref(), starting: count(&here) }];
            targets.extend(hosts.iter().filter(|(name, _)| name != source).map(|(name, ssh)| Target { name, ssh: Some(ssh), room: remote_rooms.room(name), starting: count(name) }));
            let (target, ssh) = match pick(&targets, reserved) {
                Ok(chosen) => chosen,
                Err(reasons) => {
                    let why = reasons.join("; ");
                    if printed.get(source) != Some(&why) {
                        log(&format!("not taking from {source}: {why}"));
                        printed.insert(source.clone(), why);
                    }
                    continue;
                }
            };
            printed.remove(source);
            polled = true;
            match poll(source_ssh) {
                Polled::Idle => {}
                Polled::Unreachable(why) => {
                    log(&format!("{source} unreachable: {why}"));
                    backoff = true;
                }
                Polled::Offered(child, reader, offer) => {
                    taken = true;
                    let Some(count) = starting.get(target).map(Arc::clone) else { continue };
                    count.fetch_add(1, Ordering::SeqCst);
                    serving.fetch_add(1, Ordering::SeqCst);
                    let starting = Arc::new(Starting { count, released: AtomicBool::new(false) });
                    let route = Route { source: source.clone(), target: target.to_owned(), ssh: ssh.map(str::to_owned) };
                    let serving = Arc::clone(&serving);
                    let handle = std::thread::spawn(move || {
                        serve(&route, *child, reader, *offer, starting);
                        serving.fetch_sub(1, Ordering::SeqCst);
                    });
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

/// Why this host takes no build a fleet session started here that no build host had room for: the rule it applies before running a pulled run, so a session's build and a pulled one share the interactive and game reserves.
pub fn own_build_refusal() -> Option<String> {
    if game_reserved() {
        return Some("this host holds its game reserve".to_owned());
    }
    refusal(room::read_local().as_ref(), 0)
}
