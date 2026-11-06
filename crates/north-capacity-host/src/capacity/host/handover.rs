//! The queue side of the pull hand-over: a queued agent heavy run listens on a socket beside its ticket, and `north capacity host hand-over`, which a peer runs over its own ssh connection, releases the oldest such run by relaying that connection to it. This host reaches no peer; the peer reaches it.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::args::Options;
use super::json::text;
use super::state::{ensure_state, read_queue};
use super::fail;

const POLL_MILLISECONDS: u64 = 200;
pub(super) const NOTHING_QUEUED: u8 = 3;

/// Where a ticket's run listens: named by the ticket's arrival time and first uuid group, since a Unix socket path holds 107 bytes.
pub(super) fn socket_path(root: &Path, ticket: &str) -> PathBuf {
    let stem: String = ticket.split('-').take(2).collect::<Vec<_>>().join("-");
    root.join("handover").join(format!("{stem}.sock"))
}

/// A queued run's open door for a taking peer, closed when the run leaves the queue.
pub(super) struct Listening {
    listener: tokio::net::UnixListener,
    path: PathBuf,
}

impl Listening {
    pub(super) fn open(root: &Path, ticket: &str) -> Option<Self> {
        let path = socket_path(root, ticket);
        std::fs::create_dir_all(path.parent()?).ok()?;
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).ok()?;
        Some(Self { listener, path })
    }

    pub(super) async fn accept(&self) -> Option<UnixStream> {
        let (stream, _) = self.listener.accept().await.ok()?;
        let stream = stream.into_std().ok()?;
        stream.set_nonblocking(false).ok()?;
        Some(stream)
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The queued runs a peer may take, oldest first: agent heavy runs that opened a door.
pub(super) fn pullable(queue: &[Value]) -> Vec<&str> {
    let mut names: Vec<&str> = queue
        .iter()
        .filter(|ticket| ticket.get("pullable").and_then(Value::as_bool) == Some(true) && text(ticket, "requestedClass") == Some("heavy"))
        .filter_map(|ticket| text(ticket, "name"))
        .collect();
    names.sort_unstable();
    names
}

/// Connects to the oldest queued run that answers, so the run is released to this caller alone.
pub(super) fn release(root: &Path) -> Option<(String, UnixStream)> {
    ensure_state(root);
    let queue = read_queue(root);
    pullable(&queue).into_iter().find_map(|name| UnixStream::connect(socket_path(root, name)).ok().map(|stream| (name.to_owned(), stream)))
}

fn standard(descriptor: std::os::fd::BorrowedFd<'_>) -> Option<File> {
    descriptor.try_clone_to_owned().ok().map(File::from)
}

/// Relays the connection between the caller's ssh channel and the released run until either side ends.
fn relay(stream: UnixStream) -> u8 {
    let (Some(mut stdin), Some(mut stdout)) = (standard(std::io::stdin().as_fd()), standard(std::io::stdout().as_fd())) else { return 1 };
    let Ok(mut inward) = stream.try_clone() else { return 1 };
    let mut outward = stream;
    let feeder = std::thread::spawn(move || {
        let mut buffer = [0u8; 64 * 1024];
        while let Ok(read) = stdin.read(&mut buffer) {
            if read == 0 || inward.write_all(&buffer[..read]).is_err() {
                break;
            }
        }
        let _ = inward.shutdown(std::net::Shutdown::Both);
    });
    let mut buffer = [0u8; 64 * 1024];
    let mut status = 0;
    while let Ok(read) = outward.read(&mut buffer) {
        if read == 0 {
            break;
        }
        if stdout.write_all(&buffer[..read]).is_err() {
            status = 1;
            break;
        }
    }
    let _ = outward.shutdown(std::net::Shutdown::Both);
    drop(feeder);
    status
}

/// `hand-over [--wait-seconds N]`: releases the oldest queued agent heavy run to the caller and relays its connection on stdin and stdout; exits 3 when no run is queued within the wait.
pub(super) fn run(root: &Path, argv: &[String]) -> u8 {
    let options = Options::parse(argv, 1, &["--wait-seconds"], &[]);
    if options.separator != argv.len() {
        fail("hand-over accepts no command", 2);
    }
    let wait = options.get("--wait-seconds").map_or(0, |text| text.parse::<u64>().unwrap_or_else(|_| fail("--wait-seconds takes whole seconds", 2)));
    let deadline = Instant::now() + Duration::from_secs(wait);
    loop {
        if let Some((_, stream)) = release(root) {
            return relay(stream);
        }
        if Instant::now() >= deadline {
            return NOTHING_QUEUED;
        }
        std::thread::sleep(Duration::from_millis(POLL_MILLISECONDS));
    }
}
