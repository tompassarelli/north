use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use north_base::foundation::offload::{TREES_ENV, UNPROVED};

/// A secret for one shipping of a run, which the client sends the host at the head of the job's standard input and never in an argument, the environment or a file; the host opens the line naming the tree it proved the run on with it, so no line of the check's own output passes for that proof.
pub(super) fn secret() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom").and_then(|mut random| random.read_exact(&mut bytes)).map_err(|error| format!("/dev/urandom: {error}"))?;
    Ok(bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

/// The tree a proof line names, when it names a full object id and nothing else.
pub(super) fn tree(rest: &str) -> Option<String> {
    let tree = rest.trim();
    (matches!(tree.len(), 40 | 64) && tree.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| tree.to_owned())
}

/// Appends the tree the run of `head` was proved on, else `UNPROVED`, to the record `TREES_ENV` names, which the run's lander created, when it names one.
pub(super) fn record(head: &str, tree: Option<&str>) -> Result<(), String> {
    let Some(path) = std::env::var_os(TREES_ENV).filter(|path| !path.is_empty()) else { return Ok(()) };
    let line = format!("{} {head}\n", tree.unwrap_or(UNPROVED));
    OpenOptions::new().append(true).open(&path).and_then(|mut file| file.write_all(line.as_bytes())).map_err(|error| format!("cannot record the tree of {head} in {}: {error}", Path::new(&path).display()))
}
