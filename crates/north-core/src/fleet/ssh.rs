use std::path::Path;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::platform::process::run_with_input_within;

use super::link::quote;
use super::runtime::binary;

pub(crate) const SSH_WAIT: Duration = Duration::from_secs(20);

pub(crate) fn over_ssh(host: &str, words: &[&str]) -> NorthResult<Vec<u8>> {
    over_ssh_within(host, words, None, SSH_WAIT)
}

pub(crate) fn over_ssh_within(host: &str, words: &[&str], input: Option<&[u8]>, wait: Duration) -> NorthResult<Vec<u8>> {
    over_ssh_via(&binary("ssh"), host, words, input, wait)
}

pub(crate) fn over_ssh_via(ssh: &Path, host: &str, words: &[&str], input: Option<&[u8]>, wait: Duration) -> NorthResult<Vec<u8>> {
    let command = format!("north {}", words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
    let argv = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, "--", &command];
    match run_with_input_within(&ssh.to_string_lossy(), &argv, input, wait) {
        Some(output) if output.status.success() => Ok(output.stdout),
        Some(output) => Err(NorthError::Usage(format!("ssh {host} {command}: {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        None => Err(NorthError::Usage(format!("ssh {host} {command}: no answer within {}s", wait.as_secs()))),
    }
}
