use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

/// A unix socket's peer as read through the pidfd the socket gave at connect: `read` is what the reader took from `/proc/PID`, and the process was still alive after it, so a reused pid never answered for the caller.
pub struct Held<T> {
    pub uid: u32,
    pub pid: i32,
    pub read: T,
}

/// Holds the peer of `stream` by its pidfd (SO_PEERPIDFD), runs `read` on the pid that pidfd names, and returns it only when the process is still alive afterwards; `None` when the pidfd cannot be had or the process has exited.
pub fn hold<T>(stream: &UnixStream, read: impl FnOnce(i32) -> T) -> Option<Held<T>> {
    let uid = rustix::net::sockopt::get_socket_peercred(stream).ok()?.uid.as_raw();
    let pidfd = nix::sys::socket::getsockopt(stream, nix::sys::socket::sockopt::PeerPidfd).ok()?;
    let pid = pid_of(&pidfd)?;
    let read = read(pid);
    alive(&pidfd).then_some(Held { uid, pid, read })
}

/// Whether process `pid` runs in this process's user namespace; one that cannot be read counts as outside.
pub fn shares_user_namespace(pid: i32) -> bool {
    let namespace = |process: &str| std::fs::read_link(format!("/proc/{process}/ns/user")).ok();
    namespace(&pid.to_string()).is_some_and(|theirs| Some(theirs) == namespace("self"))
}

fn pid_of(pidfd: &OwnedFd) -> Option<i32> {
    let fdinfo = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd())).ok()?;
    fdinfo.lines().find_map(|line| line.strip_prefix("Pid:"))?.trim().parse().ok().filter(|pid| *pid > 0)
}

fn alive(pidfd: &OwnedFd) -> bool {
    let mut exited = [rustix::event::PollFd::new(pidfd, rustix::event::PollFlags::IN)];
    rustix::event::poll(&mut exited, 0).is_ok_and(|ready| ready == 0)
}
