use crate::capacity::carry;
use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const TRANSPORT_FAILED: u8 = 255;
const WRONG_TREE: u8 = 98;
const NO_VERDICT: u8 = 75;
const BESIDE_MISSING: u8 = NO_VERDICT;
const SLOTS_BUSY: u8 = 96;
const WAITED: &str = "offload: waited ";
const FREE_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);
const LOCK_WAIT_SECONDS: u32 = 600;
pub const SLOTS: u32 = 6;
const SCCACHE_FIRST_PORT: u32 = 20000;
const SCCACHE_PORTS: u32 = 20000;
pub(super) const HEADROOM: (u32, u32) = (4, 5);
const POLL_SECONDS: &str = "0.2";
const TARGET_CAP_MIB: u32 = 40 * 1024;
const TARGET_FLOOR_MIB: u32 = 30 * 1024;
pub const OWNER_ENV: &str = "NORTH_OFFLOAD_OWNER";
pub const RED_FIX_ENV: &str = "NORTH_LAND_RED_FIX";
pub const LANDER_SLOT_ENV: &str = "NORTH_OFFLOAD_LANDER_SLOT";
pub(super) const OFFLOAD_ROOT: &str = "${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}";

const FREE: &str = r#"set -eu
root=@ROOT@ name=$1 free=0 i=0
mkdir -p "$root"
while [ "$i" -lt @SLOTS@ ]; do
  s=$name
  [ "$i" -eq 0 ] || s=$name-$i
  if (exec 9>"$root/$s.lock"; flock -n 9); then free=$((free + 1)); fi
  i=$((i + 1))
done
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

mod beside;
mod script;
pub use script::{SlotClass, is_lander_slot};
pub(crate) use script::slot_ports;
use script::remote_script;
#[cfg(test)]
mod pinned;

fn ensure_shipped(host: &str, sha: &str, present: impl FnOnce() -> bool, ship: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if present() {
        return Ok(());
    }
    ship().map_err(|why| format!("shipping the lander runtime {sha} to {host}: {why}"))
}

fn bundle(land: &Land, reference: &str) -> Result<File, String> {
    let git = || {
        let mut command = Command::new("git");
        command.current_dir(land.checkout);
        command
    };
    let file = tempfile::tempfile().map_err(|error| format!("bundle file: {error}"))?;
    let made = git().args(["update-ref", reference, land.head.as_str()]).status().is_ok_and(|status| status.success());
    if !made {
        return Err(format!("cannot name {} for the bundle", land.head.as_str()));
    }
    let mut command = git();
    command.args(["bundle", "create", "-q", "-", reference]);
    match &land.base {
        Some(base) if *base == land.head => command.arg(format!("^{}^", base.as_str())),
        Some(base) => command.arg(format!("^{}", base.as_str())),
        None => &mut command,
    };
    let bundled = command.stdout(file.try_clone().map_err(|error| error.to_string())?).status().is_ok_and(|status| status.success());
    let _ = git().args(["update-ref", "-d", reference]).status();
    if !bundled {
        return Err("git bundle failed".to_owned());
    }
    let mut file = file;
    file.rewind().map_err(|error| error.to_string())?;
    Ok(file)
}

/// The landing context a land check carries to the offload host as `NAME='value'` words, naming the shipped base and head; empty for any other run.
fn carried(land: &Land, value: impl Fn(&str) -> Option<String>) -> String {
    if value(carry::LANDING).as_deref() != Some("1") {
        return String::new();
    }
    let words = carry::names(carry::Boundary::Offload).filter_map(|name| {
        let value = match name {
            carry::LAND_BASE => Some(land.base.as_ref().map_or("", CommitId::as_str).to_owned()),
            carry::LAND_HEAD => Some(land.head.as_str().to_owned()),
            _ => value(name),
        };
        value.map(|value| format!("{name}={}", quote(&value)))
    });
    words.collect::<Vec<_>>().join(" ")
}

pub(super) fn run(land: &Land) -> u8 {
    let (relative, name) = match checkout(land.checkout) {
        Ok(found) => found,
        Err(why) => return unreached(&why),
    };
    let reference = job_ref(&land.head);
    let bundle = match bundle(land, &reference) {
        Ok(file) => file,
        Err(why) => return unreached(&why),
    };
    let name = land.class.slot_name(&name);
    let script = remote_script(&name, land.class);
    let own = crate::runtime::own_runtime();
    if let Some((sha, directory)) = &own
        && let Err(why) = ensure_shipped(land.ssh, sha, || crate::runtime::staged_on(land.ssh, sha), || crate::runtime::stage_on(land.ssh, sha, directory))
    {
        return unreached(&why);
    }
    let runtime = own.map_or_else(String::new, |(sha, _)| sha);
    let base = land.base.as_ref().map_or("", CommitId::as_str);
    let owner = std::env::var(OWNER_ENV).unwrap_or_default();
    let cpus = (crate::capacity::host::land_check_cpus().ceil() as u32).to_string();
    let red_fix = std::env::var(RED_FIX_ENV).unwrap_or_default();
    let lander_slot = std::env::var(LANDER_SLOT_ENV).unwrap_or_default();
    let marker = ran_marker(&reference);
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let beside = beside::pin(land.command, &relative, &home, |checkout| CommitId::resolve(checkout, "HEAD").ok());
    let words = [relative.as_str(), base, land.head.as_str(), &land.timeout_seconds.to_string(), &name, &beside.command, &reference, &carried(land, |name| std::env::var(name).ok()), &owner, &runtime, &cpus, &red_fix, &marker, &beside.pins, &lander_slot].map(quote);
    let remote = format!("setsid -w sh -c {} offload {}", quote(&script), words.join(" "));
    let spawned = with_caller("ssh")
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", "-o", crate::platform::ssh::OWN_CONNECTION, land.ssh, "--", &remote])
        .stdin(Stdio::from(bundle))
        .stdout(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return unreached(&format!("ssh: {error}")),
    };
    let ran = child.stdout.take().and_then(|stdout| relay(std::io::BufReader::new(stdout), &mut std::io::stdout(), &marker));
    let exited = match child.wait() {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()),
        Err(error) => return unreached(&format!("ssh: {error}")),
    };
    verdict(exited, ran).unwrap_or_else(|why| unreached(&why))
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

/// Copies the remote's stdout through to `out` without the status line the remote script prints after the command it ran, and returns that status.
fn relay(remote: impl std::io::BufRead, out: &mut impl std::io::Write, marker: &str) -> Option<u8> {
    let tag = format!("{marker} ");
    let mut ran = None;
    for line in remote.split(b'\n') {
        let Ok(line) = line else { break };
        let reported = String::from_utf8_lossy(&line).rfind(&tag).and_then(|at| std::str::from_utf8(&line[at + tag.len()..]).ok()?.parse::<u8>().ok().map(|code| (at, code)));
        let shown = match reported {
            Some((at, code)) => {
                ran = Some(code);
                &line[..at]
            }
            None => &line[..],
        };
        if reported.is_none() || !shown.is_empty() {
            let _ = out.write_all(shown).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush());
        }
    }
    ran
}

/// The run's exit code: the status the remote script reported for the command it ran, else the script's own refusal before running it; a run whose remote never reported its command's exit (a dropped connection, ssh exit 255, an exit before the report) reached no verdict, never the command's failure.
fn verdict(exited: Option<u8>, ran: Option<u8>) -> Result<u8, String> {
    match (exited, ran) {
        (_, Some(code)) => Ok(code),
        (Some(code @ (WRONG_TREE | BESIDE_MISSING | SLOTS_BUSY)), None) => Ok(code),
        (exited, None) => Err(format!("the connection ended ({}) before the remote host reported the command's exit, so the run reached no verdict", exited.map_or_else(|| "ssh was killed by a signal".to_owned(), |code| format!("ssh exit {code}")))),
    }
}

pub(super) fn free_slots(ssh: &str, directory: &Path) -> Result<u32, String> {
    let (_, name) = checkout(directory)?;
    let script = FREE.replace("@ROOT@", OFFLOAD_ROOT).replace("@SLOTS@", &SLOTS.to_string());
    let remote = format!("sh -c {} offload-free {}", quote(&script), quote(&name));
    let output = crate::platform::process::run_within("ssh", &["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &remote], FREE_LIMIT).ok_or_else(|| format!("{ssh} did not count its slots within {} s", FREE_LIMIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("{ssh} could not count its slots: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse().map_err(|_| format!("{ssh} counted its slots as {:?}", text.trim()))
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
