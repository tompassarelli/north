//! Runs oracle commands side by side, each under a time limit, and reads one number from each.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_base::platform::process::run_group_within;

pub const LIMIT: Duration = Duration::from_secs(8);

pub fn measure(root: &Path, oracle: &str) -> Result<f64, String> {
    let mut command = Command::new("bash");
    command.args(["-o", "pipefail", "-c", oracle]).current_dir(root);
    let Some(output) = run_group_within(command, LIMIT) else {
        return Err(format!("did not run or gave no answer within {} s", LIMIT.as_secs()));
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let said = stderr.lines().chain(stdout.lines()).find(|line| !line.trim().is_empty()).unwrap_or("no output");
        return Err(format!("exited {}: {}", output.status.code().map_or_else(|| "on a signal".into(), |code| code.to_string()), said.trim()));
    }
    let line = stdout.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim();
    line.parse::<f64>().ok().filter(|value| value.is_finite()).ok_or_else(|| format!("printed {line:?}, not one number"))
}

pub fn measure_all<'a>(oracles: &'a [(&'a Path, &'a str)]) -> Vec<Result<f64, String>> {
    std::thread::scope(|scope| {
        let running: Vec<_> = oracles.iter().map(|(root, oracle)| scope.spawn(move || measure(root, oracle))).collect();
        running.into_iter().map(|run| run.join().unwrap_or_else(|_| Err("oracle thread panicked".into()))).collect()
    })
}
