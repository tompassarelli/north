//! Spare ssh channels to a build host, opened before they are needed: `north ssh-spare hold DESTINATION` keeps a few
//! shells there waiting for a command and hands one's pipes to the next caller through a Unix socket, so a remote
//! command costs the one round trip of sending it instead of a channel open, an exec and its answer (0.23 s against
//! 0.55 to 0.9 s from the control host to a build host on 2026-11-12). A spare is an idle shell and holds no credential.
//! The holder exits once no spare is taken for `IDLE` or the runtime it started from is no longer the current one, and
//! ends the spares it still holds. It may use rustix, and its test north-base's bounded wait.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IoSlice, IoSliceMut, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, poll};
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, recvmsg, sendmsg};

const USAGE: &str = "Usage: north ssh-spare hold DESTINATION";
const SPARES: usize = 6;
const IDLE: Duration = Duration::from_secs(600);
const WATCH: Duration = Duration::from_secs(2);
const RETRY: Duration = Duration::from_secs(5);
const HANDED_WITHIN: Duration = Duration::from_millis(500);
const EXIT_MARK: &str = "north-spare-exit";
const WAITING: &str = r#"exec sh -c 'echo ready; IFS= read -r n && c=$(dd bs=1 count="$n" 2>/dev/null) && eval "$c"'"#;

/// A spare's two pipes, handed to the caller: what it writes to `input` reaches the remote shell's standard input, and `output` carries that shell's standard output until it exits.
pub struct Taken {
    pub input: File,
    pub output: File,
}

struct Spare {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

/// Runs `script` on `destination` through a spare and returns its exit status and standard output, or None when no spare
/// was ready, the shell ended before it said its status, or `within` passed; the caller then runs it the usual way.
pub fn run(destination: &str, script: &str, within: Duration) -> Option<(i32, String)> {
    run_bytes(destination, script, within).map(|(code, out)| (code, String::from_utf8_lossy(&out).into_owned()))
}

/// Runs `script` as [`run`] does and returns its standard output byte for byte, for output that need not be text.
pub fn run_bytes(destination: &str, script: &str, within: Duration) -> Option<(i32, Vec<u8>)> {
    let taken = take(destination)?;
    finished(taken, script, within)
}

/// Starts `script` on `destination` through a spare and returns its standard output to read as it comes, or None when
/// no spare was ready; the remote shell runs the script to its end whether or not the caller reads it all.
pub fn started(destination: &str, script: &str) -> Option<File> {
    let Taken { mut input, output } = take(destination)?;
    input.write_all(framed(script).as_bytes()).ok()?;
    Some(output)
}

/// A ready spare to `destination` from its holder, or None when none is ready; with no holder running, starts one for the next caller.
pub fn take(destination: &str) -> Option<Taken> {
    let socket = socket(&runtime_dir()?, destination);
    match UnixStream::connect(&socket) {
        Ok(stream) => received(&stream),
        Err(_) => {
            start_holder(destination);
            None
        }
    }
}

/// The command a caller writes to a spare's input to have it run `script`.
pub fn framed(script: &str) -> String {
    format!("{}\n{script}", script.len())
}

/// `north ssh-spare hold DESTINATION`, with `ssh` the ssh command up to and including the destination.
pub fn hold(arguments: &[String], ssh: impl Fn(&str) -> Vec<OsString> + Send + Sync + 'static) -> u8 {
    let [verb, destination] = arguments else {
        eprintln!("{USAGE}");
        return 2;
    };
    if verb != "hold" {
        eprintln!("{USAGE}");
        return 2;
    }
    let Some(dir) = runtime_dir() else { return 1 };
    let words = ssh(destination);
    match holding(&dir, destination, move || spawn(&words), current_runtime) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("north ssh-spare: {destination}: {error}");
            1
        }
    }
}

fn holding(dir: &Path, destination: &str, open: impl Fn() -> io::Result<Child> + Send + Sync + 'static, runtime: fn() -> Option<PathBuf>) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    owner_only(dir)?;
    let socket = socket(dir, destination);
    let lock = OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(socket.with_extension("lock"))?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    owner_only(&socket)?;
    let open: Arc<dyn Fn() -> io::Result<Child> + Send + Sync> = Arc::new(open);
    let (opened, arrivals) = mpsc::channel();
    for _ in 0..SPARES {
        refill(&opened, &open);
    }
    let (started_on, mut pool, mut last, mut checked) = (runtime(), Vec::new(), Instant::now(), Instant::now());
    loop {
        pool.extend(arrivals.try_iter());
        if checked.elapsed() > WATCH {
            checked = Instant::now();
            for _ in 0..prune(&mut pool) {
                refill(&opened, &open);
            }
            if last.elapsed() > IDLE || runtime() != started_on {
                let _ = fs::remove_file(&socket);
                for Spare { mut child, .. } in pool {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Ok(());
            }
        }
        if !called(&listener) {
            continue;
        }
        let Ok((stream, _)) = listener.accept() else { continue };
        pool.extend(arrivals.try_iter());
        for _ in 0..prune(&mut pool) {
            refill(&opened, &open);
        }
        let Some(Spare { mut child, input, output }) = pool.pop() else { continue };
        last = Instant::now();
        if hand(&stream, &input, &output).is_err() {
            let _ = child.kill();
        }
        drop((input, output));
        std::thread::spawn(move || child.wait());
        refill(&opened, &open);
    }
}

/// Refuses a path any other user could reach: the socket hands whoever connects a shell on the build host.
fn owner_only(path: &Path) -> io::Result<()> {
    let found = fs::symlink_metadata(path)?;
    let mode = found.permissions().mode() & 0o777;
    if mode & 0o077 == 0 && found.uid() == rustix::process::getuid().as_raw() {
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} has mode {mode:o} or another owner, so it is not this user's alone", path.display())))
}

fn called(listener: &UnixListener) -> bool {
    let mut fds = [PollFd::new(listener, PollFlags::IN)];
    let wait = i32::try_from(WATCH.as_millis()).unwrap_or(i32::MAX);
    poll(&mut fds, wait).is_ok_and(|ready| ready > 0)
}

fn prune(pool: &mut Vec<Spare>) -> usize {
    let before = pool.len();
    pool.retain_mut(|spare| matches!(spare.child.try_wait(), Ok(None)));
    before - pool.len()
}

fn refill(opened: &mpsc::Sender<Spare>, open: &Arc<dyn Fn() -> io::Result<Child> + Send + Sync>) {
    let (opened, open) = (opened.clone(), Arc::clone(open));
    std::thread::spawn(move || {
        loop {
            if let Some(spare) = open().ok().and_then(waiting) {
                let _ = opened.send(spare);
                return;
            }
            std::thread::sleep(RETRY);
        }
    });
}
fn waiting(mut child: Child) -> Option<Spare> {
    let (Some(input), Some(mut output)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        return None;
    };
    let mut line = Vec::new();
    let mut byte = [0];
    while line.len() < 16 && output.read_exact(&mut byte).is_ok() && byte[0] != b'\n' {
        line.push(byte[0]);
    }
    if line != b"ready" {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    Some(Spare { child, input, output })
}

fn hand(stream: &UnixStream, input: &ChildStdin, output: &ChildStdout) -> io::Result<()> {
    let fds = [input.as_fd(), output.as_fd()];
    let mut space = [0; rustix::cmsg_space!(ScmRights(2))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        return Err(io::Error::other("the hand-over message had no room for the spare's pipes"));
    }
    sendmsg(stream, &[IoSlice::new(b"s")], &mut control, SendFlags::empty())?;
    Ok(())
}

fn received(stream: &UnixStream) -> Option<Taken> {
    stream.set_read_timeout(Some(HANDED_WITHIN)).ok()?;
    let mut byte = [0];
    let mut space = [0; rustix::cmsg_space!(ScmRights(2))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    recvmsg(stream, &mut [IoSliceMut::new(&mut byte)], &mut control, RecvFlags::CMSG_CLOEXEC).ok()?;
    let mut fds: Vec<OwnedFd> = control.drain().filter_map(|message| match message {
        RecvAncillaryMessage::ScmRights(fds) => Some(fds),
        _ => None,
    }).flatten().collect();
    let output = fds.pop()?;
    let input = fds.pop()?;
    Some(Taken { input: File::from(input), output: File::from(output) })
}

fn finished(taken: Taken, script: &str, within: Duration) -> Option<(i32, Vec<u8>)> {
    let Taken { mut input, mut output } = taken;
    let body = format!("{script}\nprintf '\\n{EXIT_MARK} %s\\n' \"$?\"\n");
    input.write_all(framed(&body).as_bytes()).ok()?;
    drop(input);
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut read = Vec::new();
        let _ = output.read_to_end(&mut read);
        let _ = send.send(read);
    });
    let mut read = receive.recv_timeout(within).ok()?;
    let end = read.iter().rposition(|byte| *byte != b'\n').map_or(0, |last| last + 1);
    let mark = format!("\n{EXIT_MARK} ");
    let at = read[..end].windows(mark.len()).rposition(|window| window == mark.as_bytes())?;
    let code = std::str::from_utf8(&read[at + mark.len()..end]).ok()?.trim().parse().ok()?;
    read.truncate(at);
    Some((code, read))
}

fn spawn(words: &[OsString]) -> io::Result<Child> {
    let (program, arguments) = words.split_first().ok_or_else(|| io::Error::other("no ssh command"))?;
    Command::new(program).args(arguments).arg("--").arg(WAITING).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
}

fn start_holder(destination: &str) {
    let Ok(program) = std::env::current_exe() else { return };
    if program.file_name().is_none_or(|name| name != "north") {
        return;
    }
    let _ = Command::new(program).args(["ssh-spare", "hold", destination]).process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()).map(|dir| PathBuf::from(dir).join("north-spares"))
}

fn socket(dir: &Path, destination: &str) -> PathBuf {
    let name: String = destination.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    dir.join(format!("{name}.sock"))
}

fn current_runtime() -> Option<PathBuf> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let state = env.get("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map_or_else(|| PathBuf::from(env.get("HOME").map_or("", String::as_str)).join(".local/state"), PathBuf::from);
    fs::canonicalize(state.join("north/current")).ok()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
