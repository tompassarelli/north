use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn run<S: AsRef<OsStr>>(
    program: &str,
    dir: Option<&Path>,
    args: &[S],
) -> std::io::Result<Output> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.output()
}

pub(crate) async fn run_killing_on_drop(program: &str, args: &[String]) -> std::io::Result<Output> {
    tokio::process::Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
}

pub(crate) fn run_within(program: &str, args: &[&str], limit: Duration) -> Option<Output> {
    let output = tempfile::tempfile().ok()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(output.try_clone().ok()?)
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let mut output = output;
    output.rewind().ok()?;
    let mut stdout = Vec::new();
    output.read_to_end(&mut stdout).ok()?;
    Some(Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

pub(crate) fn run_with_input(program: &str, dir: &Path, args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().ok()?;
    writer.join().ok()?.ok()?;
    output.status.success().then_some(output.stdout)
}
