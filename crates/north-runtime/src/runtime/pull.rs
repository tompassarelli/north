use super::bounded;
use super::layout::{self, Layout, PROGRAMS};
use super::remote::limit;
use super::store::references;
use north_base::platform::process::child_within;
use north_offload::land_offload;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

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
    realise.args(["--realise", "--quiet"]).args(&references).stdout(Stdio::null());
    step("nix-store --realise", &mut realise, limit(REALISE))
}

/// Copies `files` of the runtime `target` on `host` into `into`, patching programs from the live runtime with zstd when `host` holds the same basis and falling back to compressed rsync.
pub(super) fn copy(host: &str, target: &str, into: &Path, files: &[&str]) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|error| error.to_string())?;
    let deadline = limit(COPY);
    let basis = basis_for(target);
    let mut remaining = Vec::new();
    for &file in files {
        let patched = basis
            .as_ref()
            .filter(|_| PROGRAMS.contains(&file))
            .is_some_and(|(sha, dir)| patch_program(host, target, sha, &dir.join(file), &into.join(file), file, deadline));
        if !patched {
            remaining.push(file);
        }
    }
    if remaining.is_empty() {
        return Ok(());
    }
    let mut copy = Command::new("rsync");
    copy.args(["-a", "-z", "--timeout=60", "-e", "ssh -o BatchMode=yes -o ConnectTimeout=20 -o ControlPath=none"])
        .args(remaining.iter().map(|file| format!("{host}:.local/state/north/runtimes/{target}/{file}")))
        .arg(into);
    step("rsync", &mut copy, deadline)
}

fn basis_for(target: &str) -> Option<(String, PathBuf)> {
    if !layout::is_sha(target) {
        return None;
    }
    let layout = Layout::from_env();
    let basis = layout.live_sha().filter(|live| live != target && layout::is_sha(live))?;
    let dir = layout.runtime(&basis);
    layout::has_programs(&dir).then_some((basis, dir))
}

fn patch_program(host: &str, target: &str, basis: &str, local_basis: &Path, dest: &Path, name: &str, deadline: Duration) -> bool {
    let Ok(hash) = layout::binary_hash(local_basis) else { return false };
    let script = format!(
        "b=.local/state/north/runtimes/{basis}/{name}; t=.local/state/north/runtimes/{target}/{name}; [ -f \"$b\" ] && [ -f \"$t\" ] && [ \"$(sha256sum <\"$b\" | cut -d' ' -f1)\" = {hash} ] && exec zstd -q -19 --long=27 --patch-from=\"$b\" \"$t\" -c"
    );
    let mut remote = Command::new("ssh");
    remote
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "-o", "ControlPath=none", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4", "--", host, &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut ssh_child) = remote.spawn() else { return false };
    let Some(stream) = ssh_child.stdout.take() else {
        reap(&mut ssh_child);
        return false;
    };
    apply_patch_stream(&mut ssh_child, stream, local_basis, dest, deadline)
}

fn apply_patch_stream(ssh_child: &mut Child, stream: ChildStdout, local_basis: &Path, dest: &Path, deadline: Duration) -> bool {
    let staged = dest.with_extension("patch-tmp");
    let _ = fs::remove_file(&staged);
    let mut decode = Command::new("zstd");
    decode
        .args(["-q", "-d", "--long=27"])
        .arg(format!("--patch-from={}", local_basis.display()))
        .arg("-f")
        .arg("-o")
        .arg(&staged)
        .stdin(stream)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(mut zstd_child) = decode.spawn() else {
        reap(ssh_child);
        return false;
    };
    let started = Instant::now();
    let decoded = child_within(&mut zstd_child, deadline).ok().flatten().is_some_and(|status| status.success());
    let fetched = child_within(ssh_child, deadline.saturating_sub(started.elapsed())).ok().flatten().is_some_and(|status| status.success());
    let non_empty = fs::metadata(&staged).is_ok_and(|meta| meta.len() > 0);
    if !(decoded && fetched && non_empty) {
        let _ = fs::remove_file(&staged);
        return false;
    }
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).and_then(|()| fs::rename(&staged, dest)).is_ok()
}

fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn step(name: &str, command: &mut Command, limit: Duration) -> Result<(), String> {
    match bounded::status(command, limit) {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{name} failed: {status}")),
        Err(error) => Err(format!("{name} did not run to completion: {error}")),
    }
}
