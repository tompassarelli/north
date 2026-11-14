//! Runs oracle commands side by side, each under a time limit, and reads one number from each; an oracle that gives no number is a fault that keeps its own output.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_base::platform::process::run_group_within;

pub const LIMIT: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    pub why: String,
    pub output: String,
}

pub fn read(root: &Path, oracle: &str) -> Result<f64, Fault> {
    let mut command = Command::new("bash");
    command.args(["-o", "pipefail", "-c", oracle]).current_dir(root);
    let Some(output) = run_group_within(command, LIMIT) else {
        return Err(Fault { why: format!("did not run or gave no answer within {} s", LIMIT.as_secs()), output: String::new() });
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let said = format!("{stderr}{stdout}").trim().to_owned();
    if !output.status.success() {
        let first = stderr.lines().chain(stdout.lines()).find(|line| !line.trim().is_empty()).unwrap_or("no output");
        return Err(Fault { why: format!("exited {}: {}", output.status.code().map_or_else(|| "on a signal".into(), |code| code.to_string()), first.trim()), output: said });
    }
    let line = stdout.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim();
    line.parse::<f64>().ok().filter(|value| value.is_finite()).ok_or_else(|| Fault { why: format!("printed {line:?}, not one number"), output: said })
}

pub fn read_all<'a>(oracles: &'a [(&'a Path, &'a str)]) -> Vec<Result<f64, Fault>> {
    std::thread::scope(|scope| {
        let running: Vec<_> = oracles.iter().map(|(root, oracle)| scope.spawn(move || read(root, oracle))).collect();
        running.into_iter().map(|run| run.join().unwrap_or_else(|_| Err(Fault { why: "oracle thread panicked".into(), output: String::new() }))).collect()
    })
}

pub fn measure_all<'a>(oracles: &'a [(&'a Path, &'a str)]) -> Vec<Result<f64, String>> {
    read_all(oracles).into_iter().map(|reading| reading.map_err(|fault| fault.why)).collect()
}
