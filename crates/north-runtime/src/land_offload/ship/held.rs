use std::io::Write as _;
use std::process::{Command, Stdio};

use north_base::foundation::offload;

use super::{CommitId, FREE_LIMIT, Land, quote};
use crate::platform::process::run_within;

/// The commits the host's checkout of the repository holds at a ref tip that this checkout also holds, the cut a bundle the host can verify needs when it lacks the shipped base.
pub(super) fn commits(land: &Land, relative: &str) -> Result<Vec<CommitId>, String> {
    let remote = format!("git -C \"$HOME\"/{} for-each-ref --format='%(objectname)'", quote(relative));
    let listed = run_within(&offload::ssh(), &["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", land.ssh, "--", &remote], FREE_LIMIT).ok_or_else(|| format!("{} did not list its checkout's refs within {} s", land.ssh, FREE_LIMIT.as_secs()))?;
    if !listed.status.success() {
        return Err(format!("{} could not list its checkout's refs: {}", land.ssh, String::from_utf8_lossy(&listed.stderr).trim()));
    }
    let mut tips: Vec<String> = String::from_utf8_lossy(&listed.stdout).split_whitespace().map(str::to_owned).collect();
    tips.sort();
    tips.dedup();
    let mut check = Command::new(offload::git()).current_dir(land.checkout).args(["cat-file", "--batch-check=%(objectname) %(objecttype)"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|error| format!("git: {error}"))?;
    let asked: String = tips.iter().map(|tip| format!("{tip}\n")).collect();
    let stdin = check.stdin.take();
    let output = std::thread::scope(|scope| {
        scope.spawn(|| stdin.map(|mut stdin| stdin.write_all(asked.as_bytes())));
        check.wait_with_output()
    })
    .map_err(|error| format!("git: {error}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.strip_suffix(" commit")).map(|sha| CommitId(sha.to_owned())).collect())
}
