use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::platform::process::run_within;

use super::link::quote;
use super::runtime::binary;

const SSH_WAIT: Duration = Duration::from_secs(20);

pub(crate) fn over_ssh(host: &str, words: &[&str]) -> NorthResult<Vec<u8>> {
    over_ssh_within(host, words, SSH_WAIT)
}

pub(crate) fn over_ssh_within(host: &str, words: &[&str], wait: Duration) -> NorthResult<Vec<u8>> {
    let command = format!("north {}", words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
    let ssh = binary("ssh");
    let argv = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, "--", &command];
    match run_within(&ssh.to_string_lossy(), &argv, wait) {
        Some(output) if output.status.success() => Ok(output.stdout),
        Some(output) => Err(NorthError::Usage(format!("ssh {host} {command}: {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        None => Err(NorthError::Usage(format!("ssh {host} {command}: no answer within {}s", wait.as_secs()))),
    }
}
