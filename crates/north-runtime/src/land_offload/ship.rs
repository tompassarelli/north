use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use bundle::{bundle, carried};
use relay::{relay, verdict};
pub(super) use north_base::foundation::exit::TRANSPORT_FAILED;
const WRONG_TREE: u8 = 98;
use north_base::foundation::exit::{NO_VERDICT, SLOTS_BUSY};
const BESIDE_MISSING: u8 = NO_VERDICT;
const RUNTIME_MISSING: u8 = 95;
const WAITED: &str = "offload: waited ";
const RUNNING: &str = "offload: running ";
const FREE_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);
const LOCK_WAIT_SECONDS: u32 = 600;
pub const SLOTS: u32 = 6;
const SCCACHE_FIRST_PORT: u32 = 20000;
const SCCACHE_PORTS: u32 = 20000;
const POLL_SECONDS: &str = "0.2";
const TARGET_CAP_MIB: u32 = 40 * 1024;
const TARGET_FLOOR_MIB: u32 = 30 * 1024;
const TARGET_KEEP_MIB: u32 = 24 * 1024;
use north_base::foundation::offload::{LANDER_SLOT_ENV, OWNER_ENV, RED_FIX_ENV};
pub(super) const OFFLOAD_ROOT: &str = "${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}";

pub(super) fn job_mib() -> u32 {
    crate::capacity::admission::dispatch_request_mib(crate::capacity::admission::Class::Heavy) as u32
}

pub(super) fn host_job_cap(cores: u32, per_check: u32, memory_mib: u32) -> u32 {
    let by_cores = if per_check <= 1 { cores } else { cores / per_check };
    let by_memory = match job_mib() {
        0 => by_cores,
        job if memory_mib < job => by_cores,
        job => memory_mib / job,
    };
    by_cores.min(by_memory).max(1)
}

const FREE: &str = r#"set -eu
root=@ROOT@ name=$1 per=${2:-0}
mkdir -p "$root"
@JOBS@@CAP@own=$(slot_jobs "$name")
free=$((@SLOTS@ - own))
room=$((cap - own)) host_room=$((host_cap - $(host_jobs)))
[ "$room" -le "$host_room" ] || room=$host_room
[ "$room" -ge 0 ] || room=0
[ "$free" -le "$room" ] || free=$room
echo "$free"
"#;

const REAP: &str = r#"owner=$1
root=@ROOT@
[ -d "$root" ] || exit 0
for f in "$root"/*.owner; do
  [ -f "$f" ] || continue
  [ "$(cat "$f")" = "$owner" ] || continue
  base=${f%.owner}
  pgid=$(cat "$base.pgid" 2>/dev/null) || pgid=
  case $pgid in "" | *[!0-9]*) ;; *)
    if tr "\\0" " " <"/proc/$pgid/cmdline" 2>/dev/null | grep -q offload; then
      kill -TERM "-$pgid" 2>/dev/null || true
      sleep 2
      kill -KILL "-$pgid" 2>/dev/null || true
      echo "offload: reaped ${base##*/} (process group $pgid)"
    fi ;;
  esac
  rm -f "$f" "$base.pgid" "$base.holder" "$base.bundle"
done
"#;

/// ```compile_fail
/// let _: north_runtime::land_offload::ship::CommitId = "HEAD".into();
/// ```
///
/// ```no_run
/// let _ = north_runtime::land_offload::ship::CommitId::resolve(std::path::Path::new("/"), "HEAD");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitId(String);

impl CommitId {
    pub fn resolve(checkout: &Path, name: &str) -> Result<Self, String> {
        let output = Command::new("git").current_dir(checkout).args(["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).output().map_err(|error| format!("git: {error}"))?;
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() && !sha.is_empty() { Ok(Self(sha)) } else { Err(format!("{name} names no commit in {}", checkout.display())) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

fn checkout(directory: &Path) -> Result<(String, String), String> {
    let output = Command::new("git").current_dir(directory).args(["rev-parse", "--path-format=absolute", "--git-common-dir"]).output().map_err(|error| format!("git: {error}"))?;
    let common = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let main = common.parent().ok_or("the repository has no main checkout")?;
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is unset")?;
    let relative = remote_checkout(main, &home)?;
    Ok((relative.clone(), relative.replace('/', "-")))
}

/// The checkout a build host keeps for the repository whose git directory sits in `main`: `code/PROJECT/main` for a project under `~/code`, whatever layout this host gave it, since the remote host keeps each project's main checkout there; else the same path relative to home.
pub(crate) fn remote_checkout(main: &Path, home: &Path) -> Result<String, String> {
    let relative = main.strip_prefix(home).map_err(|_| format!("{} is not under {}, so the remote host cannot be assumed to keep it", main.display(), home.display()))?;
    let mut parts = relative.components();
    match (parts.next(), parts.next()) {
        (Some(code), Some(project)) if code.as_os_str() == "code" => Ok(format!("code/{}/main", project.as_os_str().to_string_lossy())),
        _ => Ok(relative.to_string_lossy().into_owned()),
    }
}

pub(super) struct Land<'a> {
    pub(super) ssh: &'a str,
    pub(super) checkout: &'a Path,
    pub(super) base: Option<CommitId>,
    pub(super) head: CommitId,
    pub(super) timeout_seconds: u64,
    pub(super) command: &'a str,
    pub(super) class: SlotClass,
}

fn fail(why: &str) -> u8 {
    eprintln!("north capacity offload: {why}");
    TRANSPORT_FAILED
}

fn unreached(why: &str) -> u8 {
    eprintln!("north capacity offload: {why}");
    NO_VERDICT
}

fn job_ref(head: &CommitId) -> String {
    static JOBS: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    format!("refs/land-offload/{}-{}-{nanos}-{}", head.as_str(), std::process::id(), JOBS.fetch_add(1, Ordering::Relaxed))
}

pub(super) mod beside;
mod script;
pub(super) use script::jobs_part;
pub use script::{LANDER_JOBS, SlotClass, is_lander_slot, others_cap};
pub(crate) use script::slot_ports;
use script::remote_script;
mod bundle;
#[cfg(test)]
mod pinned;
mod relay;
mod master;
mod reattach;

pub(super) fn run(land: &Land) -> u8 {
    let (relative, name) = match checkout(land.checkout) {
        Ok(found) => found,
        Err(why) => return unreached(&why),
    };
    let own = crate::runtime::own_runtime();
    let reported = match (attempt(land, &relative, &name, own.as_ref().map(|(sha, _)| sha.as_str())), &own) {
        (Ok((Some(RUNTIME_MISSING), None)), Some((sha, directory))) if !land.ssh.is_empty() => {
            if let Err(why) = crate::runtime::stage_on(land.ssh, sha, directory) {
                return unreached(&format!("shipping the lander runtime {sha} to {}: {why}", land.ssh));
            }
            attempt(land, &relative, &name, Some(sha))
        }
        (first, _) => first,
    };
    match reported {
        Ok((exited, ran)) => verdict(exited, ran).unwrap_or_else(|why| unreached(&why)),
        Err(code) => code,
    }
}

/// One shipping of the job to the host: the remote script's exit and the status it reported for the command, or this host's failure to ship it.
fn attempt(land: &Land, relative: &str, name: &str, runtime: Option<&str>) -> Result<(Option<u8>, Option<u8>), u8> {
    let reference = job_ref(&land.head);
    let bundle = bundle(land, &reference).map_err(|why| unreached(&why))?;
    let name = land.class.slot_name(name);
    let script = remote_script(&name, land.class);
    let base = land.base.as_ref().map_or("", CommitId::as_str);
    let owner = std::env::var(OWNER_ENV).unwrap_or_default();
    let cpus = (crate::capacity::host::land_check_cpus().ceil() as u32).to_string();
    let red_fix = std::env::var(RED_FIX_ENV).unwrap_or_default();
    let lander_slot = std::env::var(LANDER_SLOT_ENV).unwrap_or_default();
    let marker = ran_marker(&reference);
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let beside = beside::pin(land.command, relative, &home, |checkout| CommitId::resolve(checkout, "HEAD").ok());
    let words = [relative, base, land.head.as_str(), &land.timeout_seconds.to_string(), &name, &beside.command, &reference, &carried(land, |name| std::env::var(name).ok()), &owner, runtime.unwrap_or_default(), &cpus, &red_fix, &marker, &beside.pins, &lander_slot];
    let mut waiting = crate::capacity::deadline::SlotWait::begin(&crate::capacity::local_machine::runtime_root());
    let master = master::options(land.ssh);
    let options: Vec<&str> = ["-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4"].into_iter().chain(master.iter().map(String::as_str)).collect();
    let spawned = on_host(land.ssh, &script, &words, &options)
        .stdin(Stdio::from(bundle))
        .stdout(Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(|error| unreached(&format!("ssh: {error}")))?;
    let started = std::cell::Cell::new(false);
    let ran = child.stdout.take().and_then(|stdout| {
        relay(std::io::BufReader::new(stdout), &mut std::io::stdout(), &marker, || {
            waiting.end();
            started.set(true);
        })
    });
    let exited = child.wait().map_err(|error| unreached(&format!("ssh: {error}")))?;
    let first = (exited.code().and_then(|code| u8::try_from(code).ok()), ran);
    Ok(reattach::ride_out(first, started.get() && !land.ssh.is_empty(), reattach::WINDOW, reattach::PAUSE, || read_back(land, &marker)))
}

/// One fresh connection to the host that reads the result of the run marked `marker` back: the output it wrote since the last byte this host sent, then its status, which the run's own script recorded in files when the first connection was lost.
fn read_back(land: &Land, marker: &str) -> (Option<u8>, Option<u8>) {
    let remote = format!("sh -c {} offload-reattach {} {}", quote(&script::reattach_script()), quote(marker), land.timeout_seconds);
    let master = master::options(land.ssh);
    let mut command = with_caller("ssh");
    command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4"]).args(&master).args([land.ssh, "--", &remote]);
    let Ok(mut child) = command.stdout(Stdio::piped()).spawn() else { return (None, None) };
    let ran = child.stdout.take().and_then(|stdout| relay(std::io::BufReader::new(stdout), &mut std::io::stdout(), marker, || {}));
    let exited = child.wait().ok().and_then(|status| status.code()).and_then(|code| u8::try_from(code).ok());
    (exited, ran)
}

fn on_host(ssh: &str, script: &str, words: &[&str], options: &[&str]) -> Command {
    if ssh.is_empty() {
        let mut command = with_caller("setsid");
        command.args(["-w", "sh", "-c", script, "offload"]).args(words);
        return command;
    }
    let remote = format!("setsid -w sh -c {} offload {}", quote(script), words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
    let mut command = with_caller("ssh");
    command.args(["-T", "-o", "BatchMode=yes"]).args(options).args([ssh, "--", &remote]);
    command
}

/// `program` set to die with this process when `setpriv` is on PATH: an ssh client orphaned by a killed caller stops relaying the remote output, so the remote job blocks writing it and holds its slot until its deadline.
fn with_caller(program: &str) -> Command {
    let setpriv = std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|directory| directory.join("setpriv").is_file()));
    if !setpriv {
        return Command::new(program);
    }
    let mut command = Command::new("setpriv");
    command.args(["--pdeathsig", "TERM", "--", program]);
    command
}

fn ran_marker(reference: &str) -> String {
    format!("north-offload-ran:{}", reference.rsplit('/').next().unwrap_or(reference))
}

pub(super) fn free_slots(ssh: &str, directory: &Path) -> Result<u32, String> {
    let (_, name) = checkout(directory)?;
    let script = FREE.replace("@JOBS@", &script::jobs_part()).replace("@CAP@", script::CAP).replace("@ROOT@", OFFLOAD_ROOT).replace("@SLOTS@", &SLOTS.to_string()).replace("@JOB_MIB@", &job_mib().to_string());
    let per = (crate::capacity::host::land_check_cpus().ceil() as u32).to_string();
    let remote = format!("sh -c {} offload-free {} {per}", quote(&script), quote(&name));
    let (counted, host) = if ssh.is_empty() {
        (crate::platform::process::run_within("sh", &["-c", &script, "offload-free", &name, &per], FREE_LIMIT), "this host")
    } else {
        (crate::platform::process::run_within("ssh", &["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &remote], FREE_LIMIT), ssh)
    };
    let output = counted.ok_or_else(|| format!("{host} did not count its slots within {} s", FREE_LIMIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("{host} could not count its slots: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse().map_err(|_| format!("{host} counted its slots as {:?}", text.trim()))
}

pub(super) fn reap(ssh: &str, owner: &str) -> u8 {
    let script = REAP.replace("@ROOT@", OFFLOAD_ROOT);
    let remote = format!("sh -c {} offload-reap {}", quote(&script), quote(owner));
    let sent = Command::new("ssh").args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2", ssh, "--", &remote]).status();
    match sent {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(TRANSPORT_FAILED),
        Err(error) => fail(&format!("ssh: {error}")),
    }
}

#[cfg(test)]
mod tests;

