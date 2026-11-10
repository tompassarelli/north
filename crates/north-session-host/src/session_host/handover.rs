//! A session host moving onto a promoted runtime without dropping its session (docs/rationale/runtime-follow.md):
//! it winds its tasks down to a quiet point, passes its provider's pipes and its listening socket to the new image
//! over a socket pair on standard input, writes the rest to its state directory and re-execs in place, so the provider,
//! still its child under the same pid, never notices.
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, IoSlice, IoSliceMut};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;
use std::time::Duration;

use north_runtime_follow::Follow;
use rustix::net::{AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketFlags, SocketType};
use serde::{Deserialize, Serialize};
use tokio::net::UnixListener;
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use super::{Activity, HostConfig, Pipes, Provider, SessionHost, Stage, StdinQueue, Watcher};
use crate::error::{NorthError, NorthResult};

const FILE: &str = "handover.json";
const POLL: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize)]
struct Carried {
    provider: u32,
    sequence: u64,
    idle_ms: u64,
    watch: serde_json::Value,
}

/// Runs the host until its provider ends, moving it onto each runtime its link promotes on the way.
pub(super) async fn follow(mut host: SessionHost, config: &HostConfig) -> NorthResult<u8> {
    let mut follow = Some(Follow::current());
    loop {
        let ended = tokio::select! {
            () = host.provider.ended() => true,
            () = tokio::time::sleep(POLL) => false,
        };
        if ended {
            return host.wait().await;
        }
        let Some(next) = follow.as_mut().and_then(Follow::replaced) else { continue };
        let Some(moving) = follow.take() else { continue };
        let (resumed, error) = hand_over(host, config, &moving).await?;
        eprintln!("north session-host: {} stays on its runtime, since it cannot move onto {next}: {error}", config.name);
        host = resumed;
    }
}

/// Winds the host down and re-execs it; returns only when it could not, serving again with the reason.
async fn hand_over(host: SessionHost, config: &HostConfig, follow: &Follow) -> NorthResult<(SessionHost, String)> {
    let SessionHost { stdin, directory, provider, writer, stdout, stderr, server, parking, park_after, watcher, stage } = host;
    let _ = stage.send(Stage::Closing);
    let listener = match server {
        Some(server) => server.await.ok().flatten(),
        None => None,
    };
    if let Some(parking) = parking {
        let _ = parking.await;
    }
    let _ = stage.send(Stage::Handing);
    let failed = |error: tokio::task::JoinError| NorthError::State(format!("pipe task failed: {error}"));
    let fed = writer.await.map_err(failed)?;
    let (output, errors) = (stdout.await.map_err(failed)??, stderr.await.map_err(failed)??);
    let error = match (&output, &errors, &listener) {
        _ if provider.has_ended() => "its provider has exited".to_owned(),
        (Some(output), Some(errors), Some(listener)) => {
            let carried = Carried { provider: provider.pid, sequence: stdin.sequence(), idle_ms: u64::try_from(stdin.activity.idle_for().as_millis()).unwrap_or(u64::MAX), watch: watcher.hand_over() };
            exec(follow, config, &carried, [fed.input.as_fd(), output.as_fd(), errors.as_fd(), listener.as_fd()]).to_string()
        }
        _ => "its provider's output or its socket has closed".to_owned(),
    };
    let pipes = Pipes { input: fed.input, output, errors };
    let mut host = SessionHost::hold(directory, provider, pipes, (stdin, fed.lanes), watcher)?;
    if let Some(listener) = listener {
        host.serve(listener);
    }
    host.rest_when_idle(park_after);
    Ok((host, error))
}

fn exec(follow: &Follow, config: &HostConfig, carried: &Carried, held: [BorrowedFd<'_>; 4]) -> io::Error {
    if let Err(error) = adopts(follow) {
        return error;
    }
    let path = config.state_dir.join(FILE);
    let staged = path.with_extension("tmp");
    let written = serde_json::to_vec(carried).map_err(io::Error::other).and_then(|bytes| fs::write(&staged, bytes)).and_then(|()| fs::rename(&staged, &path));
    if let Err(error) = written.and_then(|()| pass(held)) {
        return error;
    }
    let error = follow.reexec_with(adopting(std::env::args_os().skip(1).collect(), &path));
    if let Err(restored) = null_stdin() {
        eprintln!("north session-host: standard input was not restored: {restored}");
    }
    error
}

/// Whether the build the link names can adopt a session, as one the link rolls back to may predate `--adopt`; its
/// session-host usage names the flag.
fn adopts(follow: &Follow) -> io::Result<()> {
    let next = follow.next()?;
    let usage = std::process::Command::new(&next).arg("session-host").stdin(std::process::Stdio::null()).output()?;
    let named = |bytes: &[u8]| String::from_utf8_lossy(bytes).contains("--adopt");
    if named(&usage.stderr) || named(&usage.stdout) {
        return Ok(());
    }
    Err(io::Error::other(format!("{} cannot adopt a session host", next.display())))
}

/// Queues `held` on a socket pair whose far end becomes standard input, the one descriptor the next image can claim
/// without unsafe code.
fn pass(held: [BorrowedFd<'_>; 4]) -> io::Result<()> {
    let (ours, theirs) = rustix::net::socketpair(AddressFamily::UNIX, SocketType::STREAM, SocketFlags::CLOEXEC, None)?;
    let mut space = [0; rustix::cmsg_space!(ScmRights(4))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&held)) {
        return Err(io::Error::other("the descriptors do not fit one message"));
    }
    rustix::net::sendmsg(&ours, &[IoSlice::new(b"h")], &mut control, SendFlags::empty())?;
    rustix::stdio::dup2_stdin(&theirs)?;
    Ok(())
}

fn adopting(mut arguments: Vec<OsString>, carried: &Path) -> Vec<OsString> {
    let flags = arguments.iter().position(|argument| argument == "--").unwrap_or(arguments.len());
    if !arguments[..flags].iter().any(|argument| argument == "--adopt") {
        arguments.splice(flags..flags, [OsString::from("--adopt"), carried.as_os_str().to_owned()]);
    }
    arguments
}

/// The host its previous image handed over: the same provider, pipes, socket, sequence and watch.
pub(super) fn adopt(config: &HostConfig, carried: &Path, watcher: Watcher) -> NorthResult<SessionHost> {
    let [input, output, errors, listener] = receive()?;
    let state: Carried = serde_json::from_slice(&fs::read(carried)?).map_err(|error| NorthError::State(format!("{}: {error}", carried.display())))?;
    let _ = fs::remove_file(carried);
    watcher.take_over(state.watch);
    let pipes = Pipes {
        input: ChildStdin::from_std(input.into())?,
        output: Some(ChildStdout::from_std(output.into())?),
        errors: Some(ChildStderr::from_std(errors.into())?),
    };
    let listener = std::os::unix::net::UnixListener::from(listener);
    listener.set_nonblocking(true)?;
    let queue = StdinQueue::open(state.sequence, Activity::since(Duration::from_millis(state.idle_ms)), watcher.clone());
    let mut host = SessionHost::hold(config.generation_dir(), Provider::watch(state.provider), pipes, queue, watcher)?;
    host.serve(UnixListener::from_std(listener)?);
    Ok(host)
}

fn receive() -> io::Result<[OwnedFd; 4]> {
    let socket = io::stdin().as_fd().try_clone_to_owned()?;
    null_stdin()?;
    let mut space = [0; rustix::cmsg_space!(ScmRights(4))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let mut byte = [0; 1];
    rustix::net::recvmsg(&socket, &mut [IoSliceMut::new(&mut byte)], &mut control, RecvFlags::CMSG_CLOEXEC)?;
    let held: Vec<OwnedFd> = control
        .drain()
        .filter_map(|message| match message {
            RecvAncillaryMessage::ScmRights(fds) => Some(fds),
            _ => None,
        })
        .flatten()
        .collect();
    held.try_into().map_err(|held: Vec<OwnedFd>| io::Error::other(format!("the previous image passed {} descriptors, not 4", held.len())))
}

fn null_stdin() -> io::Result<()> {
    Ok(rustix::stdio::dup2_stdin(File::open("/dev/null")?)?)
}
