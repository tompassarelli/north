//! The queue side of the pull hand-over: a queued agent heavy run publishes a door (a socket beside its ticket), and `north capacity host hand-over`, which a peer runs over its own ssh connection, claims the oldest door by renaming it and relays that connection to the run. This host reaches no peer; the peer reaches it.

use std::cell::Cell;
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
/// How long a run whose taker failed it waits before it publishes its door again, so one failing taker cannot spin on it.
pub(super) const REPUBLISH_AFTER: Duration = Duration::from_secs(15);

fn stem(ticket: &str) -> String {
    ticket.split('-').take(2).collect::<Vec<_>>().join("-")
}

/// Where a ticket's run listens: named by the ticket's arrival time and first uuid group, since a Unix socket path holds 107 bytes.
pub(super) fn socket_path(root: &Path, ticket: &str) -> PathBuf {
    root.join("handover").join(format!("{}.sock", stem(ticket)))
}

/// The door's name once a peer has claimed it: the rename is the claim, so two peers never take one run.
fn claimed_path(root: &Path, ticket: &str) -> PathBuf {
    root.join("handover").join(format!("{}.claimed", stem(ticket)))
}

/// Where a handed-over run's ticket waits while a peer runs it, out of the queue so this host neither admits it nor holds later runs behind it, and back in the queue with its place if the peer fails it.
fn aside_path(root: &Path, ticket: &str) -> PathBuf {
    root.join("handover").join(ticket)
}

pub(super) fn set_aside(root: &Path, ticket: &str) {
    let _ = std::fs::create_dir_all(root.join("handover"));
    let _ = std::fs::rename(root.join("queue").join(ticket), aside_path(root, ticket));
}

pub(super) fn restore(root: &Path, ticket: &str) {
    let _ = std::fs::rename(aside_path(root, ticket), root.join("queue").join(ticket));
}

/// Removes every file a run's hand-over made, for a run that leaves without dropping its door.
pub(super) fn forget(root: &Path, ticket: &str) {
    for path in [socket_path(root, ticket), claimed_path(root, ticket), aside_path(root, ticket)] {
        let _ = std::fs::remove_file(path);
    }
}

/// A queued run's door for a taking peer, closed when the run leaves the queue.
pub(super) struct Listening {
    listener: tokio::net::UnixListener,
    root: PathBuf,
    ticket: String,
    republish_at: Cell<Option<Instant>>,
}

impl Listening {
    pub(super) fn open(root: &Path, ticket: &str) -> Option<Self> {
        let path = socket_path(root, ticket);
        std::fs::create_dir_all(path.parent()?).ok()?;
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).ok()?;
        Some(Self { listener, root: root.to_path_buf(), ticket: ticket.to_owned(), republish_at: Cell::new(None) })
    }

    pub(super) async fn accept(&self) -> Option<UnixStream> {
        let (stream, _) = self.listener.accept().await.ok()?;
        self.republish_at.set(None);
        let stream = stream.into_std().ok()?;
        stream.set_nonblocking(false).ok()?;
        Some(stream)
    }

    /// The claiming peer failed this run: its door opens again after `REPUBLISH_AFTER`.
    pub(super) fn give_back(&self) {
        self.republish_at.set(Some(Instant::now() + REPUBLISH_AFTER));
    }

    /// Publishes the door again once a failed claim's wait is over; a claim whose peer never connected counts as failed.
    pub(super) fn tend(&self) {
        self.tend_at(Instant::now());
    }

    pub(super) fn tend_at(&self, now: Instant) {
        let (open, claimed) = (socket_path(&self.root, &self.ticket), claimed_path(&self.root, &self.ticket));
        if open.exists() || !claimed.exists() {
            return;
        }
        match self.republish_at.get() {
            None => self.give_back(),
            Some(at) if now >= at => {
                if std::fs::rename(&claimed, &open).is_ok() {
                    self.republish_at.set(None);
                }
            }
            Some(_) => {}
        }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        forget(&self.root, &self.ticket);
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

/// Claims the oldest queued run whose door is open and connects to it, so the run is released to this caller alone; a door whose run is gone is removed.
pub(super) fn release(root: &Path) -> Option<(String, UnixStream)> {
    ensure_state(root);
    let queue = read_queue(root);
    pullable(&queue).into_iter().find_map(|name| {
        let claimed = claimed_path(root, name);
        std::fs::rename(socket_path(root, name), &claimed).ok()?;
        match UnixStream::connect(&claimed) {
            Ok(stream) => Some((name.to_owned(), stream)),
            Err(_) => {
                let _ = std::fs::remove_file(&claimed);
                None
            }
        }
    })
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
