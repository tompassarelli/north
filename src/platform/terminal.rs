use regex::Regex;
use rustix::fs::{Mode, OFlags};
use rustix::pty::OpenptFlags;
use rustix::termios::Winsize;
use std::fs::File;
use std::io::Read;
use std::process::{Command, ExitStatus};
use std::sync::LazyLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

static CONTROL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b(\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(\x07|\x1b\\)|[@-Z\\-_])").expect("control pattern"));

fn plain(text: &str) -> String {
    CONTROL.replace_all(text, "").into_owned()
}

pub(crate) fn capture(mut command: Command, limit: Duration) -> std::io::Result<(Option<ExitStatus>, String)> {
    let leader = rustix::pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)?;
    rustix::pty::grantpt(&leader)?;
    rustix::pty::unlockpt(&leader)?;
    // Claude Code wraps output at the terminal width; a wide window keeps the session URL on one line.
    rustix::termios::tcsetwinsize(&leader, Winsize { ws_row: 50, ws_col: 400, ws_xpixel: 0, ws_ypixel: 0 })?;
    let name = rustix::pty::ptsname(&leader, Vec::new())?;
    let follower = File::from(rustix::fs::open(name.as_c_str(), OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC, Mode::empty())?);
    command.stdin(follower.try_clone()?).stdout(follower.try_clone()?).stderr(follower);
    let mut child = command.spawn()?;
    drop(command);
    let chunks = read_all(File::from(leader));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait()?;
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut bytes = Vec::new();
    while let Ok(chunk) = chunks.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        bytes.extend(chunk);
    }
    Ok((status, plain(&String::from_utf8_lossy(&bytes))))
}

fn read_all(mut leader: File) -> mpsc::Receiver<Vec<u8>> {
    let (sender, chunks) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(read @ 1..) = leader.read(&mut buffer) {
            if sender.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    chunks
}
