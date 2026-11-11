use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TOOLPATH: &str = "/home/tom/.local/bin:/home/tom/.nix-profile/bin:/run/current-system/sw/bin";

pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

pub fn run(program: &str, args: &[&str], stdin: Option<&str>) -> Ran {
    let mut command = Command::new(program);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let Ok(mut child) = command.spawn() else {
        return Ran { code: 127, out: String::new(), err: format!("{program}: could not start") };
    };
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    match child.wait_with_output() {
        Ok(output) => Ran { code: output.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&output.stdout).into_owned(), err: String::from_utf8_lossy(&output.stderr).into_owned() },
        Err(error) => Ran { code: -1, out: String::new(), err: error.to_string() },
    }
}

pub fn sh(script: &str) -> Ran {
    run("bash", &["-c", script], None)
}

pub fn ssh(host: &str, script: &str) -> Ran {
    let destination = format!("tom@{host}");
    run("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=15", &destination, "bash", "-s"], Some(script))
}

pub fn git(dir: &Path, args: &[&str]) -> Ran {
    let dir = dir.to_string_lossy();
    let mut full = vec!["-C", dir.as_ref()];
    full.extend_from_slice(args);
    run("git", &full, None)
}

pub fn must(ran: Ran, what: &str) -> Result<String, String> {
    if ran.ok() { Ok(ran.out) } else { Err(format!("{what} failed (exit {}): {}{}", ran.code, ran.err.trim(), ran.out.trim())) }
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or_default()
}

pub fn local_time(epoch: f64) -> String {
    let at = format!("@{}", epoch as i64);
    run("date", &["-d", &at, "+%Y-%m-%d %I:%M:%S %p %z"], None).out.trim().to_owned()
}

pub fn rfc3339_epoch(text: &str) -> Option<f64> {
    let (date, time) = text.trim_end_matches('Z').split_once('T')?;
    let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let mut clock = time.split(':');
    let (h, min) = (clock.next()?.parse::<i64>().ok()?, clock.next()?.parse::<i64>().ok()?);
    let seconds: f64 = clock.next()?.parse().ok()?;
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some((days * 86_400 + h * 3_600 + min * 60) as f64 + seconds)
}

pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted.get(rank).copied()
}
