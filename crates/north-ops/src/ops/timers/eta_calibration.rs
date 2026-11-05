use std::io::Write;
use std::process::{Command, Stdio};

use super::{child_failure, date, north_program, text};

const ISSUE: &str = "255";
const WINDOW: &str = "7d";

pub fn run(_arguments: &[String]) -> u8 {
    match post() {
        Ok(line) => {
            println!("eta-calibration: {line}");
            0
        }
        Err(error) => {
            eprintln!("eta-calibration: {error}");
            1
        }
    }
}

pub(super) fn comment_body(today: &str, table: &str) -> String {
    format!("Weekly ETA calibration ({today}, last {WINDOW}): stated ETA against measured active minutes of runs that reported Done, from `north worker-ledger --eta-calibration`.\n\n```\n{}\n```", table.trim_end())
}

fn post() -> Result<String, String> {
    let repo = std::env::var("ETA_CALIBRATION_REPO").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "tompassarelli/north".to_owned());
    let table = Command::new(north_program()).args(["worker-ledger", "--eta-calibration", "--since", WINDOW]).output().map_err(|error| format!("cannot run north worker-ledger: {error}"))?;
    if !table.status.success() {
        return Err(child_failure("north worker-ledger --eta-calibration", table.status.code(), &table.stderr));
    }
    let body = comment_body(&date("-I"), &text(&table));
    let mut gh = Command::new("gh")
        .args(["issue", "comment", ISSUE, "-R", &repo, "--body-file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run gh: {error}"))?;
    gh.stdin.take().ok_or("gh has no stdin")?.write_all(body.as_bytes()).map_err(|error| format!("cannot write to gh: {error}"))?;
    let output = gh.wait_with_output().map_err(|error| format!("gh: {error}"))?;
    if !output.status.success() {
        return Err(child_failure("gh issue comment", output.status.code(), &output.stderr));
    }
    Ok(format!("posted to {repo}#{ISSUE}: {}", text(&output).trim()))
}
