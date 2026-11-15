use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PAYLOAD_LIMIT: u64 = 1_048_576;
const OUTPUT_LIMIT: u64 = 65_536;
pub const UNIT: &str = "firn-system-policy";

pub fn run(arguments: &[String]) {
    let mut payload = Vec::new();
    let mut input = std::io::stdin().lock();
    let read = (&mut input).take(PAYLOAD_LIMIT + 1).read_to_end(&mut payload);
    let _ = std::io::copy(&mut input, &mut std::io::sink());
    if read.is_err() || payload.len() as u64 > PAYLOAD_LIMIT || crate::guard::activation::authoring_guards_off(UNIT) {
        return;
    }
    if let Some(output) = relay(&policy_path(), arguments, &payload) {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(&output);
        let _ = stdout.flush();
    }
}

fn policy_path() -> PathBuf {
    std::env::var_os("FIRN_SYSTEM_POLICY").filter(|path| !path.is_empty()).map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/lib/firn/cli/current/bin/firn-system-policy"),
        PathBuf::from,
    )
}

fn relay(policy: &Path, arguments: &[String], payload: &[u8]) -> Option<Vec<u8>> {
    let mut child = Command::new(policy).args(arguments).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let input = payload.to_vec();
    let feeder = std::thread::spawn(move || stdin.write_all(&input));
    let mut output = Vec::new();
    let read = child.stdout.take()?.take(OUTPUT_LIMIT + 1).read_to_end(&mut output);
    let oversized = output.len() as u64 > OUTPUT_LIMIT;
    if oversized {
        let _ = child.kill();
    }
    let status = child.wait().ok()?;
    let _ = feeder.join();
    (read.is_ok() && !oversized && status.success()).then_some(output)
}
