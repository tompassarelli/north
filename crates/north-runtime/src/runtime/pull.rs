use super::bounded;
use super::layout::{self, PROGRAMS};
use super::remote::limit;
use super::store::references;
use north_offload::land_offload;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;


const COPY: Duration = Duration::from_secs(10 * 60);
const REALISE: Duration = Duration::from_secs(10 * 60);

pub(super) fn pull(target: &str, runtime: &Path) -> bool {
    land_offload::pull_sources().iter().any(|host| pull_from(host, target, runtime))
}

pub(super) fn pull_from(host: &str, target: &str, runtime: &Path) -> bool {
    admitted(land_offload::accepts_peer_runtimes_here(), host, || fetch_reported(host, target, runtime))
}

fn admitted(accepts: bool, host: &str, fetch: impl FnOnce() -> bool) -> bool {
    if !accepts {
        eprintln!("north runtime: this host settles the fleet and takes no runtime a peer offers from {host}: it builds its own or has a build host build it");
        return false;
    }
    fetch()
}

fn fetch_reported(host: &str, target: &str, runtime: &Path) -> bool {
    let short = layout::short(target);
    match fetch(host, target, runtime, &[]) {
        Ok(()) => {
            println!("north runtime: pulled {short} built on {host} and realised its store closure");
            true
        }
        Err(why) => {
            eprintln!("north runtime: {host} has no usable {short}: {why}");
            for program in PROGRAMS {
                let _ = fs::remove_file(runtime.join(program));
            }
            false
        }
    }
}

pub(super) fn fetch(host: &str, target: &str, runtime: &Path, beside: &[&str]) -> Result<(), String> {
    let files: Vec<&str> = PROGRAMS.iter().chain(beside).copied().collect();
    copy(host, target, runtime, &files)?;
    if !layout::has_programs(runtime) {
        return Err("the copy left the programs incomplete".to_owned());
    }
    let references = references(runtime).map_err(|failure| failure.message)?;
    let mut realise = Command::new("nix-store");
    realise.args(["--realise", "--quiet"]).args(&references).stdout(std::process::Stdio::null());
    step("nix-store --realise", &mut realise, limit(REALISE))
}

/// Copies `files` of the runtime `target` on `host` into `into`, and nothing else.
pub(super) fn copy(host: &str, target: &str, into: &Path, files: &[&str]) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|error| error.to_string())?;
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--timeout=60", "-e", "ssh -o BatchMode=yes -o ConnectTimeout=20 -o ControlPath=none"])
        .args(files.iter().map(|file| format!("{host}:.local/state/north/runtimes/{target}/{file}")))
        .arg(into);
    step("rsync", &mut copy, limit(COPY))
}

fn step(name: &str, command: &mut Command, limit: Duration) -> Result<(), String> {
    match bounded::status(command, limit) {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{name} failed: {status}")),
        Err(error) => Err(format!("{name} did not run to completion: {error}")),
    }
}
