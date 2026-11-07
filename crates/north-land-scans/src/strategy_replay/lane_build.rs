//! The lane's north for the replay, built on a build host and copied into this host's head-north cache:
//! the replay reads this host's decision log, so it runs here, but the build does not.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::platform::north_binary::north_binary;

use crate::{Range, git};

const CODE: &str = "crates/";
const BUILD_SECONDS: &str = "900";
const BUILT: &str = "lane-north ";

/// Puts north built from the lane's tree into this host's head-north cache when head-north would otherwise build it here, saying where it came from; a failure leaves head-north to build it as before.
pub fn prefetch(directory: &Path, range: &Range, out: &mut impl std::io::Write) {
    let Some(tree) = clean_tree(directory, range.head) else { return };
    let cached = cache().join(&tree).join("north");
    if cached.is_file() || live_covers(directory) {
        return;
    }
    let Some(host) = build_host(directory) else { return };
    let Some(destination) = ssh_destination(&host) else { return };
    let started = std::time::Instant::now();
    let _ = writeln!(out, "strategy-replay: building the lane's north on {host} for this host's replay");
    let command = format!("./bin/head-north --code {CODE} --build && printf '{BUILT}%s\\n' \"${{XDG_CACHE_HOME:-$HOME/.cache}}/north/head-north/$(git rev-parse 'HEAD^{{tree}}')/north\"");
    let built = Command::new(north_binary())
        .current_dir(directory)
        .args(["capacity", "offload", "run", "--host", &host, "--base", range.base, "--head", range.head, "--timeout", BUILD_SECONDS, "--", &command])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output();
    let remote = built.ok().filter(|output| output.status.success()).and_then(|output| String::from_utf8_lossy(&output.stdout).lines().find_map(|line| line.strip_prefix(BUILT)).map(str::to_owned));
    let Some(remote) = remote.filter(|remote| remote.ends_with(&format!("/{tree}/north"))) else {
        let _ = writeln!(out, "strategy-replay: {host} did not build the lane's north, so this host builds it");
        return;
    };
    match copy(&destination, &remote, &cached) {
        Ok(()) => {
            let _ = writeln!(out, "strategy-replay: copied the lane's north from {host} in {} s", started.elapsed().as_secs());
        }
        Err(why) => {
            let _ = writeln!(out, "strategy-replay: copying the lane's north from {host} failed ({why}), so this host builds it");
        }
    }
}

fn clean_tree(directory: &Path, head: &str) -> Option<String> {
    let status = git::text(directory, &["status", "--porcelain", "--untracked-files=no"])?;
    let at = git::text(directory, &["rev-parse", "HEAD"])?;
    let lane = git::text(directory, &["rev-parse", &format!("{head}^{{commit}}")])?;
    if !status.trim().is_empty() || at.trim() != lane.trim() {
        return None;
    }
    git::text(directory, &["rev-parse", "HEAD^{tree}"]).map(|tree| tree.trim().to_owned())
}

fn cache() -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache");
    std::env::var_os("XDG_CACHE_HOME").filter(|cache| !cache.is_empty()).map_or_else(home, PathBuf::from).join("north/head-north")
}

fn live_covers(directory: &Path) -> bool {
    Command::new(north_binary()).current_dir(directory).args(["runtime", "covers", "--head", "HEAD", "--code", CODE]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn build_host(directory: &Path) -> Option<String> {
    let here = north_base::platform::host::name();
    let chosen = Command::new(north_binary()).current_dir(directory).args(["capacity", "offload", "choose", "--any", "--avoid", &here]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok();
    let chosen = chosen.filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|host| !host.is_empty());
    chosen.or_else(|| git::text(directory, &["config", "--file", ".safe-push", "--get", "landing.remote-hosts"])?.split_whitespace().next().map(str::to_owned)).filter(|host| *host != here)
}

fn ssh_destination(host: &str) -> Option<String> {
    let listed = Command::new(north_binary()).args(["capacity", "offload", "hosts"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&listed.stdout).lines().find_map(|line| line.split_once(' ').filter(|(name, ssh)| *name == host && !ssh.is_empty()).map(|(_, ssh)| ssh.trim().to_owned()))
}

fn copy(destination: &str, remote: &str, cached: &Path) -> Result<(), String> {
    let directory = cached.parent().ok_or("no cache directory")?;
    let part = directory.with_extension("part");
    fs::create_dir_all(directory.parent().ok_or("no cache root")?).map_err(|error| error.to_string())?;
    let file = fs::File::create(&part).map_err(|error| format!("{}: {error}", part.display()))?;
    let quoted = format!("'{}'", remote.replace('\'', "'\\''"));
    let status = Command::new("ssh").args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", destination, "--", &format!("cat {quoted}")]).stdin(Stdio::null()).stdout(file).stderr(Stdio::null()).status();
    let copied = status.map_err(|error| format!("ssh: {error}")).and_then(|status| if status.success() { Ok(()) } else { Err(format!("ssh exited {status}")) }).and_then(|()| {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&part, fs::Permissions::from_mode(0o755)).map_err(|error| error.to_string())?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        fs::rename(&part, cached).map_err(|error| error.to_string())
    });
    if copied.is_err() {
        let _ = fs::remove_file(&part);
    }
    copied
}
