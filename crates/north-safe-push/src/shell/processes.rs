use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child as Running, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use north_base::foundation::step::{Refused, Step, cancelled};
use north_base::platform::process::{exit_status, on_path};
use rustix::io::dup;
use rustix::process::{Pid, Signal, kill_process_group};
use rustix::stdio::{dup2_stderr, dup2_stdout};

const POLL: Duration = Duration::from_millis(20);
const GRACE: Duration = Duration::from_secs(10);

pub struct Child<'a> {
    pub program: &'a Path,
    pub arguments: &'a [String],
    pub directory: &'a Path,
    pub environment: &'a [(&'static str, String)],
}

pub struct Finished {
    pub step: Step,
    pub outcome: String,
}

pub trait Processes {
    fn on_path(&self, program: &str) -> bool;
    fn current_exe(&self) -> PathBuf;
    fn run_with_input(&self, program: &Path, directory: &Path, arguments: &[&str], input: &str) -> std::io::Result<Step>;
    fn run_with_outcome(&self, child: &Child) -> Finished;
    fn succeeds(&self, program: &Path, directory: &Path, arguments: &[&str]) -> Option<String>;
    fn exit_code(&self, program: &str, arguments: &[&str]) -> Option<i32>;
    fn spawn_detached(&self, program: &Path, arguments: &[&str]);
    fn capture_output(&self, run: &mut dyn FnMut()) -> String;
}

pub struct Live;

impl Processes for Live {
    fn on_path(&self, program: &str) -> bool {
        on_path(program)
    }

    fn current_exe(&self) -> PathBuf {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"))
    }

    fn run_with_input(&self, program: &Path, directory: &Path, arguments: &[&str], input: &str) -> std::io::Result<Step> {
        let mut child = Command::new(program).current_dir(directory).args(arguments).stdin(Stdio::piped()).spawn()?;
        let _ = child.stdin.take().map(|mut stdin| stdin.write_all(input.as_bytes()));
        Ok(exit_status(child.wait()))
    }

    fn run_with_outcome(&self, child: &Child) -> Finished {
        let outcome = outcome_file();
        if std::fs::File::create_new(&outcome).is_err() {
            return Finished { step: Err(Refused(1)), outcome: String::new() };
        }
        let step = run_until_cancelled(child, &outcome);
        let text = std::fs::read_to_string(&outcome).unwrap_or_default();
        let _ = std::fs::remove_file(&outcome);
        Finished { step, outcome: text }
    }

    fn succeeds(&self, program: &Path, directory: &Path, arguments: &[&str]) -> Option<String> {
        let output = Command::new(program).args(arguments).current_dir(directory).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn exit_code(&self, program: &str, arguments: &[&str]) -> Option<i32> {
        Command::new(program).args(arguments).status().ok().and_then(|status| status.code())
    }

    fn spawn_detached(&self, program: &Path, arguments: &[&str]) {
        let executable = std::fs::metadata(program).is_ok_and(|meta| meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0);
        if !executable {
            return;
        }
        let _ = Command::new("setsid").arg("-f").arg(program).args(arguments).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }

    fn capture_output(&self, run: &mut dyn FnMut()) -> String {
        let Some(mut sink) = anonymous_file() else {
            run();
            return String::new();
        };
        let (Ok(out), Ok(err)) = (dup(std::io::stdout()), dup(std::io::stderr())) else {
            run();
            return String::new();
        };
        flush_output();
        if dup2_stdout(&sink).is_err() || dup2_stderr(&sink).is_err() {
            let _ = (dup2_stdout(&out), dup2_stderr(&err));
            run();
            return String::new();
        }
        run();
        flush_output();
        let _ = (dup2_stdout(&out), dup2_stderr(&err));
        let mut bytes = Vec::new();
        let _ = sink.seek(SeekFrom::Start(0)).and_then(|_| sink.read_to_end(&mut bytes));
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

fn flush_output() {
    let _ = (std::io::stdout().flush(), std::io::stderr().flush());
}

fn anonymous_file() -> Option<std::fs::File> {
    let path = outcome_file();
    let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(file)
}

fn outcome_file() -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    std::env::temp_dir().join(format!("safe-push-outcome-{}-{nanos}", std::process::id()))
}

fn run_until_cancelled(child: &Child, outcome: &Path) -> Step {
    let mut command = Command::new(child.program);
    command.args(child.arguments).arg("--outcome").arg(outcome).current_dir(child.directory).stdin(Stdio::null()).process_group(0);
    command.envs(child.environment.iter().map(|(name, value)| (name, value)));
    let mut running = command.spawn().map_err(|_| Refused(127))?;
    loop {
        match running.try_wait() {
            Ok(Some(status)) => {
                signal_group(&running, Signal::Kill);
                return exit_status(Ok(status)).map_err(|Refused(code)| Refused(if code == 143 { 130 } else { code }));
            }
            Ok(None) if cancelled().is_err() => {
                stop(&mut running);
                return Err(Refused(130));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(_) => {
                stop(&mut running);
                return Err(Refused(1));
            }
        }
    }
}

fn stop(running: &mut Running) {
    signal_group(running, Signal::Term);
    let started = Instant::now();
    while started.elapsed() < GRACE && matches!(running.try_wait(), Ok(None)) {
        std::thread::sleep(POLL);
    }
    signal_group(running, Signal::Kill);
    let _ = running.wait();
}

fn signal_group(running: &Running, signal: Signal) {
    if let Some(leader) = i32::try_from(running.id()).ok().and_then(Pid::from_raw) {
        let _ = kill_process_group(leader, signal);
    }
}
