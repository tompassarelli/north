use std::ffi::OsString;
use std::fs::File;
use std::io::{IoSlice, IoSliceMut, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use north_base::platform::session_scope::REMOVAL_ROOTS;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, recvmsg, sendmsg};
use serde::{Deserialize, Serialize};

use super::check_request;
use super::finish_request;
use super::hold_request;
use super::lane_grant::{self, Host};
use super::offload_request;
pub(super) use super::offload_request::offload_refusal;
use super::session_join;
use super::request_checkout;
use super::request_env::{environment_for, landing, landing_choices, own_homes, own_path, request_homes, urgency};
use super::{Context, FLAG, MAKER, removable};

const DIRECTORY: &str = "north-outside";
const SOCKET: &str = "broker.sock";
pub const SOCKET_ENV: &str = "NORTH_OUTSIDE_BROKER";
const UNIT: &str = north_base::platform::session_scope::WORKER_BROKER_UNIT;
const SLICE: &str = "agent.slice";
const NAMESPACE: &str = "--namespaces";
const KNOWS: &str = "--knows";
const HANDOVER_WAIT: Duration = Duration::from_secs(20);
const LENGTH_BYTES: usize = 8;
const ANSWER_WAIT: Duration = Duration::from_secs(10);
const REQUEST_LIMIT: usize = 4 << 20;
const STDIO: usize = 3;
pub(super) const REFUSED: u8 = 126;
const SOCKET_CHECK: Duration = Duration::from_millis(100);
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const JOIN_LEASE: &str = r#"echo 0 >"$0" || echo "north worker-sandbox broker: could not join the caller's lease scope $0, so this run counts against no lease" >&2; exec "$@""#;

#[derive(Serialize, Deserialize)]
pub(super) struct Request {
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: Vec<(String, String)>,
}

/// This user's runtime directory, where the user manager's bus and the capacity admission database live.
fn runtime() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw())), PathBuf::from)
}

/// The broker's socket: the one `NORTH_OUTSIDE_BROKER` names, where verify-north serves the build it judges so a candidate's sandboxed safe-push is not run by the live runtime's broker (north#1172), else the one in the runtime directory that the worker domain lets a command connect through (sockets.rs).
fn socket() -> PathBuf {
    std::env::var_os(SOCKET_ENV).filter(|value| !value.is_empty()).map_or_else(|| runtime().join(DIRECTORY).join(SOCKET), PathBuf::from)
}

/// The requests the broker runs, each a verb of `north` that has to leave the worker's Landlock domain: depbox builds a mount namespace, which Landlock refuses to a confined process; the capacity gate puts its command in a transient scope, which needs the user manager, and the broker starts that command in a new domain of its own (`confined`); `capacity host status` and `capacity host backlog` take the admission lock, reclaim ended leases and drop dead tickets as they read, in state the worker's domain keeps unwritable; `lane unlock` and `lane stray` enforce their own removal rule; `lane sync` fast-forwards a main checkout, which unlinks files the worker's domain refuses to remove (north#1097); `--worker-sandbox --built` starts a lane's vouched build output in a new domain that still hides the logins and the sockets; `--worker-sandbox --lane` starts a lane-only session's shell command in a fresh lane-only domain that also keeps the lanes the session created since its own was built; `safe-push` runs its checks before queueing, whose tests build mount namespaces of their own (north#1094), with the host's credentials for a lane-only session (lane_grant.rs) and with its own environment for any other; `finish-check COMMAND HEAD` is no verb of `north`: the broker runs that acceptance command in the caller's lane in a new worker domain and records the verdict from its exit code for the session whose scope the caller runs in (finish_request.rs); `review-hold place|lift ...` is no verb of `north` either: the broker writes or lifts a reviewer's hold on a commit in the verdict store for the session whose scope the caller runs in, with nothing of the client's but those words (hold_request.rs); `threads` reaches a ledger kept on another host over ssh, which a worker's domain refuses (south#118), and is refused to a lane-only caller; `capacity offload` in the words `land_offload::worker_request` reads runs a command on another declared host under that host's lease, the one way a worker command reaches a build host, refused to a lane-only caller and for any host this one does not reach, and started with nothing of the client's but those words (offload_request.rs). Each of them is served only from a lane or a main checkout of a project this host keeps, or a slot the build offload recorded as one's, and refused from a directory the broker cannot place in one (request_checkout.rs). Ahead of that, `task check --done-when --box TEXT OWNER/REPO#N` from a session outside every lane-only scope starts the Done-when checker outside the caller's domain, which alone writes the verdict the caller asked for, with nothing of the client's but those words, its directory included (check_request.rs).
/// Where the command begins in the arguments of `capacity host run`: after the first `--`, when a word follows it.
fn payload(words: &[&str]) -> Option<usize> {
    words.iter().position(|word| *word == "--").map(|at| at + 1).filter(|start| *start < words.len())
}

/// `arguments` with the gate's command wrapped to start in a new worker domain: the gate runs outside the client's domain, and the command must not.
pub(super) fn confined(north: &Path, arguments: Vec<String>) -> Vec<String> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let ["capacity", "host", "run", rest @ ..] = words.as_slice() else { return arguments };
    let Some(start) = payload(rest) else { return arguments };
    let wrapper = [north.to_string_lossy().into_owned(), FLAG.to_owned(), "--program".to_owned(), "--".to_owned()];
    arguments[..3 + start].iter().cloned().chain(wrapper).chain(arguments[3 + start..].iter().cloned()).collect()
}

pub(super) fn permitted(arguments: &[String]) -> bool {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["depbox", "fetch" | "run", "--", program, ..] | [FLAG, "--built" | "--lane", "--", program, ..] => !program.is_empty(),
        ["lane", "unlock" | "stray" | "remove" | "gc" | "sync", ..] => true,
        ["home", "move", ..] | ["threads", _, ..] => true,
        ["capacity", "host", "run", rest @ ..] => payload(rest).is_some(),
        ["capacity", "host", "status" | "backlog"] => true,
        ["capacity", "offload", rest @ ..] => crate::land_offload::worker_request(rest).is_some(),
        [finish_request::VERB, ..] => true,
        [crate::runtime::review_hold::VERB, ..] => true,
        _ => lane_grant::privileged(arguments),
    }
}

/// The trees the worker command that sent a request may remove beneath, read from the arguments, directory and environment it sent, so the process the broker starts stays inside them.
pub(super) fn hand_off_roots(arguments: &[String], directory: &Path, environment: &[(String, String)]) -> Vec<PathBuf> {
    removable(&Context::from_request(&arguments.join(" "), directory, environment))
}

/// Runs `north ARGUMENTS` outside this process's Landlock domain through the broker, with this process's standard streams, working directory and environment; its exit code.
pub fn request(arguments: &[String]) -> Result<u8, String> {
    if !permitted(arguments) {
        return Err(format!("north worker-sandbox: {} is not a request the outside broker runs", arguments.join(" ")));
    }
    let path = socket();
    let connect = || UnixStream::connect(&path).map_err(|error| format!("north worker-sandbox: the outside broker at {} does not answer ({error}); it starts with the next agent command", path.display()));
    connect()?;
    if newer_shape(arguments) && !await_knowing(|| UnixStream::connect(&path).is_ok_and(|asked| knows(&asked, arguments)), HANDOVER_WAIT) {
        return Err(format!("north worker-sandbox: the outside broker at {} runs a north older than this one and did not hand over within {} s, so `north {}` was not sent", path.display(), HANDOVER_WAIT.as_secs(), arguments.iter().take(3).cloned().collect::<Vec<_>>().join(" ")));
    }
    let stream = connect()?;
    let directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let environment = std::env::vars_os().filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?))).collect();
    let (stdin, stdout, stderr) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    send(&stream, Request { arguments: arguments.to_vec(), directory, environment }, [stdin.as_fd(), stdout.as_fd(), stderr.as_fd()])
}

/// Asks the broker to run the acceptance command of a finish call (finish_request.rs) in `directory`, the caller's lane, with `log` as the command's output and nothing else of this process's: no environment is sent; the command's exit code.
pub fn request_finish_check(directory: &Path, arguments: &[String], log: &File) -> Result<u8, String> {
    let path = socket();
    let stream = UnixStream::connect(&path).map_err(|error| format!("north worker-sandbox: the outside broker at {} does not answer ({error}); it starts with the next agent command", path.display()))?;
    let stdin = File::open("/dev/null").map_err(|error| error.to_string())?;
    send(&stream, Request { arguments: arguments.to_vec(), directory: directory.to_owned(), environment: Vec::new() }, [stdin.as_fd(), log.as_fd(), log.as_fd()])
}

/// Asks the broker for a request that carries only its words, the Done-when check `arguments` name (check_request.rs) or a reviewer's hold (hold_request.rs), with this process's standard streams and nothing else of it: the request holds those words, no working directory and no environment; its exit code.
pub fn request_check(arguments: &[String]) -> Result<u8, String> {
    let path = socket();
    let stream = UnixStream::connect(&path).map_err(|error| format!("north worker-sandbox: the outside broker at {} does not answer ({error}); it starts with the next agent command", path.display()))?;
    let (stdin, stdout, stderr) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    send(&stream, Request { arguments: arguments.to_vec(), directory: PathBuf::from("/"), environment: Vec::new() }, [stdin.as_fd(), stdout.as_fd(), stderr.as_fd()])
}

/// Whether `arguments` have a shape no older than the question `knows` asks, so a broker still serving from a replaced build may never have heard it; every shape older than the question is sent as it always was.
fn newer_shape(arguments: &[String]) -> bool {
    matches!(arguments, [noun, verb, ..] if noun == "capacity" && verb == "offload")
}

/// Whether the broker on `stream` runs requests of the shape `arguments` have. A broker of an older build answers this question as it answers every request it does not run, which reads as no.
pub(super) fn knows(stream: &UnixStream, arguments: &[String]) -> bool {
    let asked: Vec<String> = std::iter::once(KNOWS.to_owned()).chain(arguments.iter().cloned()).collect();
    let Ok(body) = serde_json::to_vec(&Request { arguments: asked, directory: PathBuf::from("/"), environment: Vec::new() }) else { return false };
    let mut answer = [1u8; 1];
    let asked = stream.set_read_timeout(Some(ANSWER_WAIT)).and_then(|()| (&*stream).write_all(&(body.len() as u64).to_le_bytes())).and_then(|()| (&*stream).write_all(&body)).and_then(|()| (&*stream).read_exact(&mut answer));
    asked.is_ok() && answer[0] == 0
}

/// Waits, up to `wait`, until `knows` says a broker that runs the request listens: for the moment after a promotion the replaced build's broker still holds the socket while its successor starts, and a request it has never heard waits for the successor instead of failing there or running inside the caller's domain.
pub(super) fn await_knowing(mut knows: impl FnMut() -> bool, wait: Duration) -> bool {
    let started = std::time::Instant::now();
    loop {
        if knows() {
            return true;
        }
        if started.elapsed() >= wait {
            return false;
        }
        std::thread::sleep(SOCKET_CHECK);
    }
}

/// Sends `request` with `streams` as its standard streams over `stream` and waits for its exit code.
pub(super) fn send(stream: &UnixStream, request: Request, streams: [BorrowedFd; STDIO]) -> Result<u8, String> {
    let body = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let mut space = [0u8; 64];
    let mut control = SendAncillaryBuffer::new(&mut space);
    control.push(SendAncillaryMessage::ScmRights(&streams));
    let length = (body.len() as u64).to_le_bytes();
    sendmsg(stream, &[IoSlice::new(&length)], &mut control, SendFlags::empty()).map_err(|error| error.to_string())?;
    (&*stream).write_all(&body).map_err(|error| error.to_string())?;
    let mut code = [0u8; 1];
    (&*stream).read_exact(&mut code).map_err(|error| format!("north worker-sandbox: the outside broker closed the request without an answer ({error})"))?;
    Ok(code[0])
}

/// Starts the broker as a transient user service when nothing answers its socket. Called before a command's domain is applied: afterwards the command cannot start a service, only ask this one. A draining broker still holds the unit name `UNIT`, so when that name is taken the broker starts under a name of its own.
pub fn ensure() {
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    ensure_listening(&socket(), |unit| start(&north, unit, &[]));
}

pub(super) fn ensure_listening(path: &Path, start: impl Fn(&str) -> bool) {
    if UnixStream::connect(path).is_ok() {
        return;
    }
    let own = format!("{UNIT}-{}", std::process::id());
    for unit in [UNIT, own.as_str()] {
        if start(unit) && listening(path) {
            return;
        }
    }
}

fn listening(path: &Path) -> bool {
    for _ in 0..40 {
        if UnixStream::connect(path).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Starts `north` as the broker in the transient user service `unit` under `agent.slice`, so the checks and builds it runs for workers count against the agent CPU quota: in the default app.slice a session's brokered safe-push compiled outside the 12-CPU desk bound and held the laptop at load 39 (north#892); whether it started. A broker on a socket of its own follows the runtime link its starter follows, so the one a canary probe starts stays on the build it judges, and a successor serves what the broker it follows `served`.
pub(super) fn start(north: &Path, unit: &str, served: &[OsString]) -> bool {
    let followed = std::env::var_os(SOCKET_ENV).and_then(|_| std::env::var_os(north_runtime_follow::LINK));
    let started = Command::new("systemd-run").args(["--user", "--quiet", "--collect", "--service-type=exec", "--property=Restart=on-failure"]).arg(format!("--setenv=XDG_RUNTIME_DIR={}", runtime().display())).args(std::env::var_os(SOCKET_ENV).map(|socket| format!("--setenv={SOCKET_ENV}={}", socket.to_string_lossy()))).args(followed.map(|link| format!("--setenv={}={}", north_runtime_follow::LINK, link.to_string_lossy()))).arg(format!("--slice={SLICE}")).args(["--unit", unit, "--"]).arg(north).args([FLAG, "--broker"]).args(served).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    started.is_ok_and(|status| status.success())
}

/// Serves requests asked from this user's projects, and what `served` adds (verify-north's scratch project lies outside `~/code`), until its socket is replaced or removed, then exits once the requests it took are done. Once the promoted north is another one it exits when idle, and while requests run it starts the promoted north as its successor in a unit of its own, which takes the socket over: on a busy host the broker was never idle, so on 2026-11-10 it kept running a north three promotions old and new requests never met a landed fix (north#1023).
pub(super) fn serve(served: Vec<OsString>, answer: Answer) -> std::process::ExitCode {
    let path = socket();
    let Some(directory) = path.parent() else { return std::process::ExitCode::from(2) };
    let north = match std::env::current_exe() {
        Ok(north) => north,
        Err(error) => {
            eprintln!("north worker-sandbox broker: {error}");
            return std::process::ExitCode::from(1);
        }
    };
    let listener = std::fs::create_dir_all(directory).and_then(|()| std::fs::remove_file(&path).or_else(|error| if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) })).and_then(|()| UnixListener::bind(&path)).and_then(|listener| listener.set_nonblocking(true).map(|()| listener));
    let listener = match listener {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("north worker-sandbox broker: cannot listen on {}: {error}", path.display());
            return std::process::ExitCode::from(1);
        }
    };
    let own = inode(&path);
    let running = std::sync::Arc::new(AtomicUsize::new(0));
    let mut handed_over = false;
    let mut follow = north_runtime_follow::Follow::current();
    loop {
        let idle = running.load(Ordering::SeqCst) == 0;
        if inode(&path) != own {
            if idle {
                return std::process::ExitCode::SUCCESS;
            }
            std::thread::sleep(SOCKET_CHECK);
            continue;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let (north, running, answer) = (north.clone(), running.clone(), answer.clone());
                running.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    if let Err(error) = handle(&north, stream, &answer) {
                        eprintln!("north worker-sandbox broker: {error}");
                    }
                    running.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if !handed_over && follow.replaced().is_some() {
                    handed_over = true;
                    if !start(&north_base::platform::north_binary::north_binary(), &format!("{UNIT}-{}", std::process::id()), &served) {
                        eprintln!("north worker-sandbox broker: cannot start the promoted north as a successor, so this one serves until it is idle");
                    }
                }
                await_request(&listener);
            }
            Err(error) => eprintln!("north worker-sandbox broker: accept: {error}"),
        }
    }
}

/// Waits until a request arrives on `listener` or [`SOCKET_CHECK`] passes, so a request is taken at once while the replaced-socket and promoted-runtime checks still run between requests: a sleep between accepts kept every worker shell call, hook and tool waiting a median 65 ms on cpx62b (north#1167).
fn await_request(listener: &UnixListener) {
    let _ = rustix::event::poll(&mut [rustix::event::PollFd::new(listener, rustix::event::PollFlags::IN)], i32::try_from(SOCKET_CHECK.as_millis()).unwrap_or(i32::MAX));
}

fn inode(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|metadata| metadata.ino())
}

pub(super) fn receive(stream: &UnixStream) -> Result<Option<(Request, Vec<OwnedFd>)>, String> {
    let mut length = [0u8; LENGTH_BYTES];
    let mut space = [0u8; 64];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(stream, &mut [IoSliceMut::new(&mut length)], &mut control, RecvFlags::CMSG_CLOEXEC).map_err(|error| error.to_string())?;
    if received.bytes == 0 {
        return Ok(None);
    }
    let streams: Vec<OwnedFd> = control.drain().filter_map(|message| if let RecvAncillaryMessage::ScmRights(fds) = message { Some(fds.collect::<Vec<_>>()) } else { None }).flatten().collect();
    let length = usize::try_from(u64::from_le_bytes(length)).map_err(|error| error.to_string())?;
    if length > REQUEST_LIMIT {
        return Err(format!("a request of {length} bytes is past the {REQUEST_LIMIT} byte limit"));
    }
    let mut body = vec![0u8; length];
    (&*stream).read_exact(&mut body).map_err(|error| error.to_string())?;
    Ok(Some((serde_json::from_slice(&body).map_err(|error| error.to_string())?, streams)))
}

/// How a broker answers the one request on a stream from a caller whose `/proc/PID/cgroup` reads as given: what it serves is its starter's to say (`broker_start`).
pub(super) type Answer = std::sync::Arc<dyn Fn(&Path, &UnixStream, Option<&str>) -> Result<(), String> + Send + Sync>;

fn handle(north: &Path, stream: UnixStream, answer: &Answer) -> Result<(), String> {
    let peer = rustix::net::sockopt::get_socket_peercred(&stream).map_err(|error| error.to_string())?;
    if peer.uid.as_raw() != rustix::process::getuid().as_raw() {
        return Err(format!("refused a request from uid {}", peer.uid.as_raw()));
    }
    let cgroup = std::fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero().get())).ok();
    answer(north, &stream, cgroup.as_deref())
}

/// Answers the one request on `stream` from a caller whose `/proc/PID/cgroup` reads `cgroup` (`None` when unreadable), on `host`; a refused or failed request is answered 126 and its reason returned for the broker's log.
pub(super) fn answer(north: &Path, stream: &UnixStream, cgroup: Option<&str>, host: &Host) -> Result<(), String> {
    let Some((request, streams)) = receive(stream)? else { return Ok(()) };
    if request.arguments == [NAMESPACE] {
        return answer_namespaces(north, stream, &request);
    }
    if let [first, asked @ ..] = request.arguments.as_slice()
        && first == KNOWS
    {
        return (&*stream).write_all(&[u8::from(!permitted(asked))]).map_err(|error| error.to_string());
    }
    let outcome = run(north, request, streams, stream, cgroup, host);
    (&*stream).write_all(&[*outcome.as_ref().unwrap_or(&REFUSED)]).map_err(|error| error.to_string())?;
    outcome.map(|_| ())
}

fn run(north: &Path, request: Request, streams: Vec<OwnedFd>, stream: &UnixStream, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    if !permitted(&request.arguments) {
        return Err(format!("{} is not a request the broker runs", request.arguments.join(" ")));
    }
    if request.arguments.first().is_some_and(|verb| verb == "threads") && cgroup.is_none_or(north_base::platform::session_scope::lane_only_cgroup) {
        return Err("a lane-only session's command does not reach the ledger host through the broker".to_owned());
    }
    if let Some(cgroup) = cgroup.filter(|cgroup| !north_base::platform::session_scope::lane_only_cgroup(cgroup))
        && let Some(asked) = check_request::asked(&request.arguments)
    {
        return run_check(north, asked, streams, stream, cgroup, host);
    }
    if let Some(asked) = crate::runtime::review_hold::Ask::asked(&request.arguments) {
        return hold_request::answer(asked, streams, cgroup, host);
    }
    let unrefused = |checkout: request_checkout::Checkout| offload_refusal(&request.arguments, cgroup.is_none_or(north_base::platform::session_scope::lane_only_cgroup), crate::land_offload::reached_destination).map_or(Ok(checkout), Err);
    let placed = request_checkout::served(&host.code, &request_checkout::Slots::of(&host.environment, &host.home), &request.directory).and_then(unrefused).and_then(|checkout| Ok((offload_request::granted(&request.arguments, &checkout, &host.environment, &runtime(), &own_homes())?, checkout)));
    let (offload, checkout) = match placed {
        Ok(placed) => placed,
        Err(why) => return Err(refuse(streams, why)),
    };
    if let Some(asked) = finish_request::asked(&request.arguments) {
        return run_finish_check(north, asked, &checkout, streams, stream, cgroup, host);
    }
    super::brokered_reads::named_file_readable(&request.arguments, &request.directory, &request.environment, cgroup)?;
    let own_home = own_homes().into_iter().find(|(name, _)| name == "HOME").map(|(_, home)| PathBuf::from(home)).unwrap_or_default();
    let viewed = super::brokered_reads::pinned_entries_readable(&request.arguments, &checkout.top, &request.directory, &request.environment, cgroup, &own_home)?;
    let [stdin, stdout, stderr]: [OwnedFd; STDIO] = streams.try_into().map_err(|_| "a request carries three standard streams".to_owned())?;
    if !request.directory.is_absolute() {
        return Err(format!("working directory {} is not absolute", request.directory.display()));
    }
    if lane_grant::granted(&request.arguments, cgroup) {
        return run_granted(north, &request, [stdout, stderr], stream, cgroup, host);
    }
    if let Some(offload) = offload {
        let offload = offload_request::Offload { environment: urgency(offload.environment, cgroup, host), ..offload };
        let started = offload.start(|arguments| brokered(north, arguments, cgroup), [stdin, stdout, stderr]);
        return wait(started.map_err(|error| format!("{}: {error}", north.display()))?, stream);
    }
    let roots = request.arguments.first().is_some_and(|word| word == "depbox").then(|| hand_off_roots(&request.arguments, &request.directory, &request.environment));
    let homes = request_homes(&request.arguments, cgroup, &request.environment, own_homes());
    let chosen = landing_choices(&request.arguments, &host.home, &host.environment, north);
    let mut variables = urgency(landing(environment_for(&request.arguments, request.environment, cgroup, &runtime(), &homes, &host.environment), chosen), cgroup, host);
    if let Some(path) = own_path(&request.arguments, &host.environment) {
        variables.retain(|(name, _)| name != "PATH");
        variables.push(path);
    }
    if let Some(commit) = viewed {
        variables.retain(|(name, _)| name != north_base::foundation::carry::LAND_STORE_VIEW);
        variables.push((north_base::foundation::carry::LAND_STORE_VIEW.to_owned(), commit));
    }
    if let Some(roots) = roots {
        variables.push((REMOVAL_ROOTS.to_owned(), serde_json::to_string(&roots).unwrap_or_else(|_| "[]".to_owned())));
    }
    variables.extend(session_join::scope_for(&request.arguments, cgroup, &host.cgroups));
    let child = brokered(north, confined(north, request.arguments.clone()), cgroup)
        .current_dir(&request.directory)
        .env_clear()
        .envs(variables)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0)
        .spawn()
        .map_err(|error| format!("{}: {error}", north.display()))?;
    wait(child, stream)
}

/// Runs the acceptance command `asked` names for the finish call of the caller's own session, in the lane the request was placed in (finish_request.rs), relaying its output to the client; the broker records the verdict from the command's exit code, never from a word of the request. A refusal is told to the client and logged.
fn run_finish_check(north: &Path, asked: Result<finish_request::Check, String>, checkout: &request_checkout::Checkout, streams: Vec<OwnedFd>, stream: &UnixStream, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    let granted = match asked.and_then(|check| finish_request::granted(check, checkout, cgroup.unwrap_or_default(), host, &runtime(), &own_homes()).map(|granted| granted.joining(north))) {
        Ok(granted) => granted,
        Err(why) => return Err(format!("refused a finish check: {}", refuse(streams, why))),
    };
    let [_, stdout, stderr]: [OwnedFd; STDIO] = streams.try_into().map_err(|_| "a request carries three standard streams".to_owned())?;
    eprintln!("north worker-sandbox broker: running the finish check of {}", granted.session);
    let code = relayed(north, &granted.arguments, &granted.directory, urgency(granted.environment.clone(), cgroup, host), [stdout, stderr], stream)?;
    match granted.record(code, &host.environment, &host.home) {
        Ok(()) => eprintln!("north worker-sandbox broker: recorded the finish verdict of {} (exit {code})", granted.session),
        Err(why) => eprintln!("north worker-sandbox broker: {why}"),
    }
    Ok(code)
}

/// Tells the client on its standard error why its request is refused; the reason, for the broker's log.
pub(super) fn refuse(streams: Vec<OwnedFd>, why: String) -> String {
    if let Some(stderr) = streams.into_iter().nth(STDIO - 1) {
        let _ = writeln!(File::from(stderr), "north worker-sandbox broker: refused: {why}");
    }
    why
}

/// The command that starts `north ARGUMENTS` for a caller whose `/proc/PID/cgroup` reads `cgroup`: inside the caller's capacity lease scope when it runs in one, so a leased build's depbox and the compiles it forks count against that lease and its host's admission. Started in the broker's own service, a leased land check's clippy and a session's depbox compiles ran outside every lease, and admission read 0.05 cores for a lease whose compiles held 7 (gex45, 2026-11-10, north#1036).
pub(super) fn brokered(north: &Path, arguments: Vec<String>, cgroup: Option<&str>) -> Command {
    let Some(scope) = cgroup.and_then(crate::capacity::gate::lease_cgroup) else {
        let mut command = Command::new(north);
        command.args(arguments);
        return command;
    };
    let mut command = Command::new("/bin/sh");
    command.args(["-c", JOIN_LEASE]).arg(Path::new(CGROUP_ROOT).join(scope.trim_start_matches('/')).join("cgroup.procs")).arg(north).args(arguments);
    command
}

/// Runs a verb `lane_grant` grants a lane-only session in its own lane, with the broker's environment and no input, relaying its output to the client's `stdout` and `stderr` with every token withheld; a refusal is told to the client and logged.
fn run_granted(north: &Path, request: &Request, [stdout, stderr]: [OwnedFd; 2], stream: &UnixStream, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    let granted = lane_grant::session(cgroup.unwrap_or_default(), &host.fleet).and_then(|session| {
        let arguments = lane_grant::grant(&session, &request.arguments, &request.directory)?;
        let variables = lane_grant::environment(&session, host.environment.iter().cloned(), &host.home)?;
        Ok((session, arguments, variables))
    });
    let (session, arguments, variables) = match granted {
        Ok(granted) => granted,
        Err(why) => {
            let _ = writeln!(File::from(stderr), "north worker-sandbox broker: refused: {why}");
            return Err(format!("refused {:?} from {}: {why}", request.arguments.first().map(String::as_str).unwrap_or_default(), request.directory.display()));
        }
    };
    eprintln!("north worker-sandbox broker: granted {} to {} ({})", arguments.iter().take(3).cloned().collect::<Vec<_>>().join(" "), session.name, session.task);
    relayed(north, &arguments, &session.lane, urgency(variables, cgroup, host), [stdout, stderr], stream)
}

/// Runs the Done-when check `asked` names for a caller outside every lane-only scope whose `/proc/PID/cgroup` reads `cgroup` (check_request.rs), relaying the checker's output to the client; a refusal is told to the client and logged.
fn run_check(north: &Path, asked: Result<check_request::Check, String>, streams: Vec<OwnedFd>, stream: &UnixStream, cgroup: &str, host: &Host) -> Result<u8, String> {
    let granted = match asked.and_then(|check| check_request::granted(&check, cgroup, host, &runtime(), &own_homes())) {
        Ok(granted) => granted,
        Err(why) => return Err(format!("refused a check: {}", refuse(streams, why))),
    };
    let [_, stdout, stderr]: [OwnedFd; STDIO] = streams.try_into().map_err(|_| "a request carries three standard streams".to_owned())?;
    eprintln!("north worker-sandbox broker: checking {} for {}", granted.arguments.last().map(String::as_str).unwrap_or_default(), granted.session);
    relayed(north, &granted.arguments, &granted.directory, urgency(granted.environment, Some(cgroup), host), [stdout, stderr], stream)
}

/// Starts `north ARGUMENTS` in `directory` with `variables` and no input, relaying its output to the client's `stdout` and `stderr` with every token withheld; its exit code.
fn relayed(north: &Path, arguments: &[String], directory: &Path, variables: Vec<(String, String)>, [stdout, stderr]: [OwnedFd; 2], stream: &UnixStream) -> Result<u8, String> {
    let mut child = Command::new(north)
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .envs(variables)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("{}: {error}", north.display()))?;
    let relays: Vec<_> = [child.stdout.take().map(|out| Box::new(out) as Box<dyn Read + Send>), child.stderr.take().map(|err| Box::new(err) as Box<dyn Read + Send>)]
        .into_iter()
        .zip([stdout, stderr])
        .filter_map(|(from, to)| from.map(|from| std::thread::spawn(move || lane_grant::relay(from, File::from(to)))))
        .collect();
    let code = wait(child, stream);
    for relay in relays {
        let _ = relay.join();
    }
    code
}

/// The exit code of `child`, whose process group is killed when the client hangs up `stream`.
fn wait(mut child: Child, stream: &UnixStream) -> Result<u8, String> {
    let group = rustix::process::Pid::from_raw(child.id() as i32);
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status.code().map_or_else(|| 128 + status.signal().unwrap_or(0) as u8, |code| code as u8));
        }
        if hung_up(stream) {
            if let Some(group) = group {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Kill);
            }
            let _ = child.wait();
            return Err("the client hung up; its process group was killed".to_owned());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether the client end of `stream` is gone: it sends nothing after its request, so an end of stream is a closed one.
fn hung_up(stream: &UnixStream) -> bool {
    rustix::net::recv(stream, &mut [0u8; 1], RecvFlags::PEEK | RecvFlags::DONTWAIT).is_ok_and(|read| read == 0)
}

/// What a namespace maker laid its overlay over: the sockets it hid and the file `/etc/ssh/ssh_config` resolved to, which a NixOS activation moves to a new store path the old overlay does not cover (north#1083).
pub(super) struct Laid {
    pub(super) hidden: Vec<PathBuf>,
    pub(super) ssh_config: Option<PathBuf>,
    pub(super) inodes: Vec<(u64, u64)>,
}

impl Laid {
    fn now(runtime: &Path, request: &Request) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let agent = request.environment.iter().find(|(name, _)| name == super::sockets::AGENT_ENV).map(|(_, value)| PathBuf::from(value));
        let hidden = super::sockets::hidden(runtime, &multiplexer(request), &super::sockets::ssh_reach(&home, rustix::process::getuid().as_raw(), agent.as_deref()));
        let inodes = hidden.iter().filter_map(|path| std::fs::symlink_metadata(path).ok()).map(|metadata| (metadata.dev(), metadata.ino())).collect();
        Self { hidden, ssh_config: super::sockets::ssh_config(), inodes }
    }

    /// Whether namespaces laid for `self` still serve `now`: they hide every socket there is now, each the very socket they laid the null device over, since a socket made again where an overlaid one was unlinked sheds that overlay (an ssh master that restarts does so), and overlay the ssh configuration ssh reads now.
    pub(super) fn serves(&self, now: &Self) -> bool {
        self.ssh_config == now.ssh_config && now.hidden.iter().all(|path| self.hidden.contains(path)) && now.inodes.iter().all(|inode| self.inodes.contains(inode))
    }
}

struct Namespaces {
    laid: Laid,
    user: OwnedFd,
    mount: OwnedFd,
}

static NAMESPACES: Mutex<Option<Namespaces>> = Mutex::new(None);

/// Asks the broker for the namespaces that hide this host's process-starting sockets and joins them. Works from inside a Landlock domain, which cannot lay the overlay itself.
pub fn join_namespaces() -> Result<(), String> {
    let stream = UnixStream::connect(socket()).map_err(|error| format!("the outside broker does not answer ({error})"))?;
    stream.set_read_timeout(Some(ANSWER_WAIT)).map_err(|error| error.to_string())?;
    let sent = ["TMUX_TMPDIR", super::sockets::AGENT_ENV].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
    let body = serde_json::to_vec(&Request { arguments: vec![NAMESPACE.to_owned()], directory: PathBuf::from("/"), environment: sent }).map_err(|error| error.to_string())?;
    (&stream).write_all(&(body.len() as u64).to_le_bytes()).and_then(|()| (&stream).write_all(&body)).map_err(|error| error.to_string())?;
    let mut answer = [0u8; 1];
    let mut space = [0u8; 64];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(&stream, &mut [IoSliceMut::new(&mut answer)], &mut control, RecvFlags::CMSG_CLOEXEC).map_err(|error| error.to_string())?;
    let fds: Vec<OwnedFd> = control.drain().filter_map(|message| if let RecvAncillaryMessage::ScmRights(fds) = message { Some(fds.collect::<Vec<_>>()) } else { None }).flatten().collect();
    let [user, mount]: [OwnedFd; 2] = fds.try_into().map_err(|_| format!("the outside broker made no namespaces ({})", if received.bytes == 0 { "it closed the request" } else { "it refused" }))?;
    super::sockets::join(user.as_fd(), mount.as_fd())
}

fn multiplexer(request: &Request) -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    super::sockets::multiplexer(uid, request.environment.iter().find(|(name, _)| name == "TMUX_TMPDIR").map(|(_, value)| value.into()))
}

/// The namespaces for the sockets and ssh configuration there are now, the last made ones when they still serve them.
fn namespaces(north: &Path, request: &Request) -> Result<[OwnedFd; 2], String> {
    let runtime = runtime();
    let now = Laid::now(&runtime, request);
    let mut held = NAMESPACES.lock().map_err(|error| error.to_string())?;
    if !held.as_ref().is_some_and(|made| made.laid.serves(&now)) {
        *held = Some(make(north, &runtime, now)?);
    }
    let made = held.as_ref().ok_or("no namespaces")?;
    Ok([made.user.try_clone().map_err(|error| error.to_string())?, made.mount.try_clone().map_err(|error| error.to_string())?])
}

fn make(north: &Path, runtime: &Path, laid: Laid) -> Result<Namespaces, String> {
    let mut maker = Command::new(north).args([FLAG, MAKER]).arg(runtime).args(&laid.hidden).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(|error| format!("{}: {error}", north.display()))?;
    let mut answer = vec![0u8; super::sockets::READY.len()];
    let ready = maker.stdout.take().ok_or("no output")?.read_exact(&mut answer).is_ok() && answer == super::sockets::READY;
    let own = |name: &str| std::fs::File::open(format!("/proc/{}/ns/{name}", maker.id())).map(OwnedFd::from).map_err(|error| format!("{name} namespace: {error}"));
    let opened = if ready { own("user").and_then(|user| own("mnt").map(|mount| Namespaces { laid, user, mount })) } else { Err("the namespace maker did not lay its overlay".to_owned()) };
    drop(maker.stdin.take());
    let _ = maker.wait();
    opened
}

fn answer_namespaces(north: &Path, stream: &UnixStream, request: &Request) -> Result<(), String> {
    match namespaces(north, request) {
        Ok(fds) => {
            let borrowed: [BorrowedFd; 2] = [fds[0].as_fd(), fds[1].as_fd()];
            let mut space = [0u8; 64];
            let mut control = SendAncillaryBuffer::new(&mut space);
            control.push(SendAncillaryMessage::ScmRights(&borrowed));
            sendmsg(stream, &[IoSlice::new(&[0])], &mut control, SendFlags::empty()).map(|_| ()).map_err(|error| error.to_string())
        }
        Err(message) => {
            let _ = (&*stream).write_all(&[1]);
            Err(message)
        }
    }
}
