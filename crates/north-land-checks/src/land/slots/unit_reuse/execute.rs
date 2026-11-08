use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DRAIN: Duration = Duration::from_secs(10);

fn pump(mut from: impl Read, mut to: impl Write, text: &Mutex<Vec<u8>>) {
    let mut buffer = [0u8; 8192];
    while let Ok(read) = from.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let _ = to.write_all(&buffer[..read]).and_then(|()| to.flush());
        if let Ok(mut held) = text.lock() {
            held.extend_from_slice(&buffer[..read]);
        }
    }
}

pub fn run(command: &str, environment: &[(String, String)]) -> std::io::Result<(i32, String)> {
    let mut child = Command::new("bash").args(["-euo", "pipefail", "-c", command]).envs(environment.iter().map(|(name, value)| (name, value))).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let text = Arc::new(Mutex::new(Vec::new()));
    let (finished, drained) = mpsc::channel();
    let mut readers = 0;
    if let Some(pipe) = child.stdout.take() {
        let (text, finished) = (Arc::clone(&text), finished.clone());
        thread::spawn(move || {
            pump(pipe, std::io::stdout(), &text);
            let _ = finished.send(());
        });
        readers += 1;
    }
    if let Some(pipe) = child.stderr.take() {
        let (text, finished) = (Arc::clone(&text), finished);
        thread::spawn(move || {
            pump(pipe, std::io::stderr(), &text);
            let _ = finished.send(());
        });
        readers += 1;
    }
    let status = child.wait()?;
    for _ in 0..readers {
        if drained.recv_timeout(DRAIN).is_err() {
            break;
        }
    }
    let code = status.code().or_else(|| status.signal().map(|signal| 128 + signal)).unwrap_or(1);
    let bytes = text.lock().map(|held| held.clone()).unwrap_or_default();
    Ok((code, String::from_utf8_lossy(&bytes).into_owned()))
}
