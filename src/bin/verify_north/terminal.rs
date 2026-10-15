use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::exec;
use super::processes::Process;

const SESSION: &str = "verify";
const TMUX_WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(200);
const QUIT_WAIT: Duration = Duration::from_secs(5);
const TAIL_LINES: usize = 12;

pub struct Size {
    pub columns: u16,
    pub rows: u16,
}

pub struct Terminal {
    socket: String,
}

impl Terminal {
    pub fn attach(socket: &str) -> Self {
        Self { socket: socket.to_owned() }
    }

    pub fn open(socket: &str, directory: &Path, size: &Size, argv: &[&str]) -> Result<Self, String> {
        let terminal = Self::attach(socket);
        let mut command = terminal.tmux(&["new-session", "-d", "-s", SESSION, "-x", &size.columns.to_string(), "-y", &size.rows.to_string(), "-c"]);
        command.arg(directory).arg("--").args(argv);
        exec::operator_environment(&mut command);
        exec::stdout(command, TMUX_WAIT, None)?;
        Ok(terminal)
    }

    pub fn size(&self) -> Result<String, String> {
        Ok(self.query("#{window_width}x#{window_height}")?.trim().to_owned())
    }

    pub fn program(&self) -> Result<Process, String> {
        let pid = self.query("#{pane_pid}")?;
        pid.trim().parse().ok().and_then(Process::of).ok_or_else(|| format!("the terminal reported no live program (pane pid {})", pid.trim()))
    }

    pub fn keys(&self, keys: &[&str]) -> Result<(), String> {
        exec::stdout(self.tmux(&[&["send-keys", "-t", SESSION], keys].concat()), TMUX_WAIT, None).map(drop)
    }

    pub fn press(&self, key: &str, times: usize) -> Result<(), String> {
        (0..times).try_for_each(|_| self.keys(&[key]))
    }

    pub fn type_text(&self, text: &str) -> Result<(), String> {
        self.keys(&["-l", text])
    }

    pub fn screen(&self) -> Result<String, String> {
        exec::stdout(self.tmux(&["capture-pane", "-p", "-t", SESSION]), TMUX_WAIT, None)
    }

    pub fn wait_for(&self, limit: Duration, what: &str, shown: impl Fn(&str) -> bool) -> Result<(), String> {
        let mut last = String::new();
        let found = exec::wait_for(limit, POLL, || {
            last = self.screen()?;
            Ok(shown(&last).then_some(()))
        })?;
        found.ok_or_else(|| format!("{what} did not appear on screen within {} s; screen ends:\n{}", limit.as_secs(), tail(&last)))
    }

    pub fn close(&self) {
        let socket_path = self.query("#{socket_path}").map(|path| PathBuf::from(path.trim()));
        let _ = self.keys(&["Escape"]);
        let _ = self.keys(&["C-q"]);
        exec::poll_until(QUIT_WAIT, POLL, || exec::stdout(self.tmux(&["has-session", "-t", SESSION]), TMUX_WAIT, None).is_err());
        let _ = exec::output(self.tmux(&["kill-server"]), TMUX_WAIT, None);
        if let Ok(path) = socket_path {
            let _ = fs::remove_file(path);
        }
    }

    fn query(&self, format: &str) -> Result<String, String> {
        exec::stdout(self.tmux(&["display-message", "-p", "-t", SESSION, format]), TMUX_WAIT, None)
    }

    fn tmux(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new("tmux");
        command.args(["-u", "-f", "/dev/null", "-L", &self.socket]).args(arguments);
        command
    }
}

pub fn tail(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).filter(|line| !line.is_empty()).collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}
