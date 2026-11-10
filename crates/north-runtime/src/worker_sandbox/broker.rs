use std::fs::File;
use std::io::{IoSlice, IoSliceMut, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use north_base::platform::session_scope::REMOVAL_ROOTS;
use north_base::platform::{git_credential, session_scope};
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, recvmsg, sendmsg};
use serde::{Deserialize, Serialize};

use super::lane_grant::{self, Host};
use super::session_join;
use super::{Context, FLAG, LANE_ONLY, MAKER, MARK, SESSION_SCOPE, SOCKETS_HIDDEN, removable};

const DIRECTORY: &str = "north-outside";
const SOCKET: &str = "broker.sock";
const UNIT: &str = "north-worker-outside";
const RUNTIME_ENV: &str = "XDG_RUNTIME_DIR";
const HOMES: [(&str, &str); 4] = [("XDG_STATE_HOME", ".local/state"), ("XDG_CACHE_HOME", ".cache"), ("XDG_DATA_HOME", ".local/share"), ("XDG_CONFIG_HOME", ".config")];
const NAMESPACE: &str = "--namespaces";
const LENGTH_BYTES: usize = 8;
const ANSWER_WAIT: Duration = Duration::from_secs(10);
const REQUEST_LIMIT: usize = 4 << 20;
const STDIO: usize = 3;
const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const JOIN_LEASE: &str = r#"echo 0 >"$0" || echo "north worker-sandbox broker: could not join the caller's lease scope $0, so this run counts against no lease" >&2; exec "$@""#;
const INJECTED_NAMES: [&str; 19] = [
    "GCONV_PATH", "LOCPATH", "NLSPATH", "HOSTALIASES", "RESOLV_HOST_CONF", "GLIBC_TUNABLES", "BASH_ENV", "ENV", "NODE_OPTIONS", "SSH_ASKPASS", "GIT_EXEC_PATH", "GIT_SSH", "GIT_SSH_COMMAND", "GIT_ASKPASS", "GIT_PAGER", "GIT_EDITOR", "GIT_EXTERNAL_DIFF", "GIT_PROXY_COMMAND", "GIT_TEMPLATE_DIR",
];
const INJECTED_PREFIXES: [&str; 6] = ["LD_", "MALLOC_", "PYTHON", "PERL5", "RUBY", "GIT_CONFIG"];

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

/// The broker's socket, in the runtime directory that the worker domain lets a command connect through (sockets.rs).
fn socket() -> PathBuf {
    runtime().join(DIRECTORY).join(SOCKET)
}

/// The requests the broker runs, each a verb of `north` that has to leave the worker's Landlock domain: depbox builds a mount namespace, which Landlock refuses to a confined process; the capacity gate puts its command in a transient scope, which needs the user manager, and the broker starts that command in a new domain of its own (`confined`); `lane unlock` and `lane stray` enforce their own removal rule; `--worker-sandbox --built` starts a lane's vouched build output in a new domain that still hides the logins and the sockets; `--worker-sandbox --lane` starts a lane-only session's shell command in a fresh lane-only domain that also keeps the lanes the session created since its own was built.
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
        ["lane", "unlock" | "stray" | "remove" | "gc", ..] => true,
        ["home", "move", ..] => true,
        ["capacity", "host", "run", rest @ ..] => payload(rest).is_some(),
        _ => lane_grant::privileged(arguments),
    }
}

fn unsafe_name(name: &str) -> bool {
    name == MARK || name == SOCKETS_HIDDEN || name == REMOVAL_ROOTS || name == SESSION_SCOPE || name.is_empty() || name.contains('=') || INJECTED_NAMES.contains(&name) || INJECTED_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The trees the worker command that sent a request may remove beneath, read from the arguments, directory and environment it sent, so the process the broker starts stays inside them.
pub(super) fn hand_off_roots(arguments: &[String], directory: &Path, environment: &[(String, String)]) -> Vec<PathBuf> {
    removable(&Context::from_request(&arguments.join(" "), directory, environment))
}

/// The XDG homes a brokered command reads and writes: this broker's own, since the command runs as the host's process and its state, cache and data belong under the host's homes. Taking the client's scratch ones would record a build's capacity admissions beside the worker instead of in the host's decision ledger (north#1030).
pub(super) fn homes(home: Option<PathBuf>, get: impl Fn(&str) -> Option<PathBuf>) -> Vec<(String, String)> {
    let home = home.unwrap_or_else(|| PathBuf::from("/"));
    HOMES
        .into_iter()
        .map(|(name, below)| (name.to_owned(), get(name).unwrap_or_else(|| home.join(below)).to_string_lossy().into_owned()))
        .collect()
}

fn own_homes() -> Vec<(String, String)> {
    fn set(name: &str) -> Option<PathBuf> {
        std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
    }
    homes(set("HOME"), set).into_iter().chain(set("HOME").map(|home| ("HOME".to_owned(), home.to_string_lossy().into_owned()))).collect()
}

/// The environment a request's process gets: the client's, without the sandbox mark, the variables that make a loader, interpreter or git run code of the client's choosing (git's program, editor, template and configuration variables), and relative or empty `PATH` entries; git's credential helper is north's own, and `XDG_RUNTIME_DIR` is `runtime`, the broker's own, since the command runs outside and reaches the user manager and the capacity database there whatever the client set or left out (an OpenCode worker clears it, north#1023). The XDG homes and `HOME` (`homes`) are the broker's own too: a client's scratch homes would take the state, cache and data a brokered build writes with it (north#1030), and `north home move` takes the broker's `HOME` as the top of its moves (north#992). Its lane-only marker follows `cgroup`, the cgroup of the process that asked (`None` when unreadable, which counts as lane-only), never the client's word: a caller in a lane-only session's scope gets the marker whether it sent one or not, and any other caller keeps one it sent, since the marker only narrows what its command reads (south#104).
pub(super) fn environment(sent: Vec<(String, String)>, cgroup: Option<&str>, runtime: &Path, homes: &[(String, String)]) -> Vec<(String, String)> {
    let lane_only = cgroup.is_none_or(session_scope::lane_only_cgroup);
    let path = |value: &str| std::env::join_paths(std::env::split_paths(value).filter(|entry| entry.is_absolute())).map(|joined| joined.to_string_lossy().into_owned()).unwrap_or_default();
    let own_runtime = (RUNTIME_ENV.to_owned(), runtime.to_string_lossy().into_owned());
    sent.into_iter().filter(|(name, _)| !unsafe_name(name) && name != RUNTIME_ENV && name != "HOME" && !HOMES.iter().any(|(own, _)| *own == name)).map(|(name, value)| if name == "PATH" { (name, path(&value)) } else { (name, value) }).chain(git_credential::session_env()).chain(lane_only.then(|| (LANE_ONLY.to_owned(), "1".to_owned()))).chain([own_runtime]).chain(homes.iter().cloned()).collect()
}

/// Runs `north ARGUMENTS` outside this process's Landlock domain through the broker, with this process's standard streams, working directory and environment; its exit code.
pub fn request(arguments: &[String]) -> Result<u8, String> {
    if !permitted(arguments) {
        return Err(format!("north worker-sandbox: {} is not a request the outside broker runs", arguments.join(" ")));
    }
    let path = socket();
    let stream = UnixStream::connect(&path).map_err(|error| format!("north worker-sandbox: the outside broker at {} does not answer ({error}); it starts with the next agent command", path.display()))?;
    let directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let environment = std::env::vars_os().filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?))).collect();
    let (stdin, stdout, stderr) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    send(&stream, Request { arguments: arguments.to_vec(), directory, environment }, [stdin.as_fd(), stdout.as_fd(), stderr.as_fd()])
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
    ensure_listening(&socket(), |unit| start(&north, unit));
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

/// Starts `north` as the broker in the transient user service `unit`; whether it started.
fn start(north: &Path, unit: &str) -> bool {
    let started = Command::new("systemd-run").args(["--user", "--quiet", "--collect", "--service-type=exec", "--property=Restart=on-failure"]).arg(format!("--setenv=XDG_RUNTIME_DIR={}", runtime().display())).args(["--unit", unit, "--"]).arg(north).args([FLAG, "--broker"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    started.is_ok_and(|status| status.success())
}

/// Serves requests until its socket is replaced or removed, then exits once the requests it took are done. Once the promoted north is another one it exits when idle, and while requests run it starts the promoted north as its successor in a unit of its own, which takes the socket over: on a busy host the broker was never idle, so on 2026-11-10 it kept running a north three promotions old and new requests never met a landed fix (north#1023).
pub fn serve() -> std::process::ExitCode {
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
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let (north, running) = (north.clone(), running.clone());
                running.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    if let Err(error) = handle(&north, stream) {
                        eprintln!("north worker-sandbox broker: {error}");
                    }
                    running.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if !handed_over && follow.replaced().is_some() {
                    handed_over = true;
                    if !start(&north_base::platform::north_binary::north_binary(), &format!("{UNIT}-{}", std::process::id())) {
                        eprintln!("north worker-sandbox broker: cannot start the promoted north as a successor, so this one serves until it is idle");
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => eprintln!("north worker-sandbox broker: accept: {error}"),
        }
    }
}

fn inode(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|metadata| metadata.ino())
}

fn receive(stream: &UnixStream) -> Result<Option<(Request, Vec<OwnedFd>)>, String> {
    let mut length = [0u8; LENGTH_BYTES];
    let mut space = [0u8; 64];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(stream, &mut [IoSliceMut::new(&mut length)], &mut control, RecvFlags::empty()).map_err(|error| error.to_string())?;
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

fn handle(north: &Path, stream: UnixStream) -> Result<(), String> {
    let peer = rustix::net::sockopt::get_socket_peercred(&stream).map_err(|error| error.to_string())?;
    if peer.uid.as_raw() != rustix::process::getuid().as_raw() {
        return Err(format!("refused a request from uid {}", peer.uid.as_raw()));
    }
    let cgroup = std::fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero().get())).ok();
    answer(north, &stream, cgroup.as_deref(), &Host::own())
}

/// Answers the one request on `stream` from a caller whose `/proc/PID/cgroup` reads `cgroup` (`None` when unreadable), on `host`; a refused or failed request is answered 126 and its reason returned for the broker's log.
pub(super) fn answer(north: &Path, stream: &UnixStream, cgroup: Option<&str>, host: &Host) -> Result<(), String> {
    let Some((request, streams)) = receive(stream)? else { return Ok(()) };
    if request.arguments == [NAMESPACE] {
        return answer_namespaces(north, stream, &request);
    }
    let outcome = run(north, request, streams, stream, cgroup, host);
    (&*stream).write_all(&[*outcome.as_ref().unwrap_or(&126)]).map_err(|error| error.to_string())?;
    outcome.map(|_| ())
}

fn run(north: &Path, request: Request, streams: Vec<OwnedFd>, stream: &UnixStream, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    if !permitted(&request.arguments) {
        return Err(format!("{} is not a request the broker runs", request.arguments.join(" ")));
    }
    super::brokered_reads::plan_readable(&request.arguments, &request.directory, &request.environment, cgroup)?;
    let [stdin, stdout, stderr]: [OwnedFd; STDIO] = streams.try_into().map_err(|_| "a request carries three standard streams".to_owned())?;
    if !request.directory.is_absolute() {
        return Err(format!("working directory {} is not absolute", request.directory.display()));
    }
    if lane_grant::privileged(&request.arguments) {
        return run_granted(north, &request, [stdout, stderr], stream, cgroup, host);
    }
    let roots = request.arguments.first().is_some_and(|word| word == "depbox").then(|| hand_off_roots(&request.arguments, &request.directory, &request.environment));
    let mut variables = environment(request.environment, cgroup, &runtime(), &own_homes());
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
    let mut child = Command::new(north)
        .args(&arguments)
        .current_dir(&session.lane)
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
}

impl Laid {
    fn now(runtime: &Path, request: &Request) -> Self {
        Self { hidden: super::sockets::hidden(runtime, &multiplexer(request)), ssh_config: super::sockets::ssh_config() }
    }

    /// Whether namespaces laid for `self` still serve `now`: they hide every socket there is now and overlay the ssh configuration ssh reads now.
    pub(super) fn serves(&self, now: &Self) -> bool {
        self.ssh_config == now.ssh_config && now.hidden.iter().all(|path| self.hidden.contains(path))
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
    let tmux = std::env::var("TMUX_TMPDIR").ok();
    let body = serde_json::to_vec(&Request { arguments: vec![NAMESPACE.to_owned()], directory: PathBuf::from("/"), environment: tmux.into_iter().map(|value| ("TMUX_TMPDIR".to_owned(), value)).collect() }).map_err(|error| error.to_string())?;
    (&stream).write_all(&(body.len() as u64).to_le_bytes()).and_then(|()| (&stream).write_all(&body)).map_err(|error| error.to_string())?;
    let mut answer = [0u8; 1];
    let mut space = [0u8; 64];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(&stream, &mut [IoSliceMut::new(&mut answer)], &mut control, RecvFlags::empty()).map_err(|error| error.to_string())?;
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
    let runtime = socket().parent().and_then(Path::parent).map(Path::to_path_buf).ok_or("the broker has no runtime directory")?;
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
