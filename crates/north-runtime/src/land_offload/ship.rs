use crate::capacity::carry;
use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const TRANSPORT_FAILED: u8 = 255;
const WRONG_TREE: u8 = 98;
const BESIDE_MISSING: u8 = 75;
const SLOTS_BUSY: u8 = 96;
const WAITED: &str = "offload: waited ";
const FREE_LIMIT: std::time::Duration = std::time::Duration::from_secs(15);
const LOCK_WAIT_SECONDS: u32 = 600;
const SLOTS: u32 = 6;
const SCCACHE_FIRST_PORT: u32 = 20000;
const SCCACHE_PORTS: u32 = 20000;
const POLL_SECONDS: &str = "0.2";
const TARGET_CAP_MIB: u32 = 40 * 1024;
const TARGET_FLOOR_MIB: u32 = 30 * 1024;
pub const OWNER_ENV: &str = "NORTH_OFFLOAD_OWNER";
pub const RED_FIX_ENV: &str = "NORTH_LAND_RED_FIX";
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

const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6 ref=$7 carried=${8:-} owner=${9:-} runtime=${10:-} per=${11:-0} red_fix=${12:-} ran=${13:-} pins=${14:-}
root=@ROOT@
mkdir -p "$root"
bundle=$(mktemp "$root/$name.bundle.XXXXXX")
trap 'rm -f "$bundle"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
trap 'exit 143' HUP INT TERM
cat >"$bundle"
if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
  git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q --no-auto-maintenance "$bundle" "+$ref:$ref"
nearest=$(
  i=0
  while [ "$i" -lt @SLOTS@ ]; do
    s=$name
    [ "$i" -eq 0 ] || s=$name-$i
    d=999999999
    if [ -d "$root/$s/target" ] && at=$(git -C "$root/$s" rev-parse -q --verify HEAD 2>/dev/null) && files=$(git -C "$repo" diff --name-only "$at" "$head" 2>/dev/null); then
      d=$(printf '%s\n' "$files" | grep -c . || true)
    fi
    echo "$d $s"
    i=$((i + 1))
  done | sort -n -s -k1,1 | cut -d ' ' -f2
)
queue=$root/queue
mkdir -p "$queue"
rank=1
[ -z "$red_fix" ] || rank=0
ticket=$queue/$rank-$(date +%s%N)-$$@$name
exec 8>"$ticket"
flock -n 8
trap 'rm -f "$bundle" "$ticket"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
cores=$(nproc 2>/dev/null || echo 1)
cap=${NORTH_OFFLOAD_MAX_JOBS:-}
if [ -z "$cap" ]; then
  cap=@SLOTS@
  [ "$per" -le 1 ] || { cap=$((cores / per)); [ "$cap" -ge 1 ] || cap=1; [ "$cap" -le @SLOTS@ ] || cap=@SLOTS@; }
fi
served=$root/served
mkdir -p "$served"
turn_order() {
  for t in $(ls "$queue"); do
    repo=; at=0
    case $t in *@*) repo=${t#*@}; [ ! -r "$served/$repo" ] || read -r at <"$served/$repo" || at=0 ;; esac
    rest=${t#*-}
    printf '%s %s %s %s\n' "${t%%-*}" "$at" "${rest%%-*}" "$t"
  done | sort -s -k1,1n -k2,2n -k3,3 | cut -d ' ' -f4
}
first_in_line() {
  now=$(date +%s%N)
  for t in $(turn_order); do
    f=$queue/$t
    [ "$f" != "$ticket" ] || return 0
    stamp=${t#*-}
    if [ $((now - ${stamp%%-*})) -gt 5000000000 ] && (exec 7>>"$f"; flock -n 7) 2>/dev/null; then rm -f "$f"; else return 1; fi
  done
  return 1
}
running_jobs() {
  n=0
  for l in "$root"/*.lock; do
    [ -e "$l" ] || continue
    (exec 7>>"$l"; flock -n 7) 2>/dev/null || n=$((n + 1))
  done
  echo "$n"
}
started=$(date +%s)
noted=0
while :; do
  if first_in_line && [ "$(running_jobs)" -lt "$cap" ]; then
    for s in $nearest; do
      exec 9>"$root/$s.lock"
      if flock -n 9; then break 2; fi
      exec 9>&-
    done
  fi
  waited=$(( $(date +%s) - started ))
  [ "$waited" -lt @LOCK_WAIT@ ] || { echo "offload: queued behind other $name jobs for @LOCK_WAIT@ s ($cap jobs run at once, @SLOTS@ slots)" >&2; exit @SLOTS_BUSY@; }
  [ "$waited" -lt "$noted" ] || { noted=$((waited + 60)); echo "offload: waiting for a slot for ${waited}s, in order, $cap jobs run at once; $(ls "$queue" | grep -c .) queued; held by: $(cat "$root/$name".holder "$root/$name"-*.holder 2>/dev/null | tr '\n' ';')" >&2; }
  sleep @POLL@
done
waited=$(( $(date +%s) - started ))
[ "$waited" -eq 0 ] || echo "@WAITED@${waited}s in line for $s" >&2
date +%s%N >"$served/$name"
rm -f "$ticket"
exec 8>&-
slot=$root/$s
# sccache keys hold absolute paths, so every slot names its own directory as the base its keys are relative to, and serves it on its own port.
case $s in @SLOT_PORTS@ esac
SCCACHE_BASEDIRS=$slot; export SCCACHE_BASEDIRS SCCACHE_SERVER_PORT
printf '%s since %s\n' "$(printf '%.12s' "$head")" "$(date +%H:%M:%S)" >"$root/$s.holder"
printf '%s\n' "$owner" >"$root/$s.owner"
printf '%s\n' "$$" >"$root/$s.pgid"
cleanup() {
  rm -f "$bundle" "$root/$s.owner" "$root/$s.pgid" "$root/$s.holder"
  git -C "$repo" update-ref -d "$ref" 2>/dev/null || true
  if [ -e "$slot/.git" ]; then git -C "$slot" clean -ffdxq -e /target 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 143' HUP INT TERM
if [ -d "$slot/target" ]; then
  used=$(du -sm "$slot/target" | cut -f1)
  free=$(df -Pm "$root" | awk 'NR == 2 { print $4 }')
  if [ "$used" -gt "${NORTH_OFFLOAD_TARGET_CAP_MIB:-@TARGET_CAP@}" ] || [ "$free" -lt "${NORTH_OFFLOAD_TARGET_FLOOR_MIB:-@TARGET_FLOOR@}" ]; then
    echo "offload: removing $s/target ($used MiB, $free MiB free on the host)" >&2
    rm -rf -- "$slot/target"
  fi
fi
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e /target
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

const VERIFY: &str = r#"landed=$(git -C "$slot" rev-parse HEAD)
[ "$landed" = "$head" ] || { echo "offload: refusing to run: $slot is on $landed, not the shipped $head" >&2; exit @WRONG_TREE@; }
echo "offload: running $head in $slot"
"#;

const BESIDE: &str = r#"beside=$root/$s.beside
for pin in $pins; do
  project=${pin%%=*} sha=${pin#*=} source=$HOME/code/${pin%%=*}/main
  [ -e "$source/.git" ] || { echo "offload: this host keeps no $source for the check to read" >&2; exit @BESIDE_MISSING@; }
  git -C "$source" cat-file -e "$sha^{commit}" 2>/dev/null || git -C "$source" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$source" cat-file -e "$sha^{commit}" 2>/dev/null || { echo "offload: $source lacks $sha, the commit the check reads on the lander" >&2; exit @BESIDE_MISSING@; }
  if [ -e "$beside/$project/.git" ]; then
    git -C "$beside/$project" checkout -q --detach -f "$sha" || exit @BESIDE_MISSING@
  else
    mkdir -p "$beside" && git -C "$source" worktree prune && git -C "$source" worktree add -q --detach "$beside/$project" "$sha" || { echo "offload: cannot pin $beside/$project to $sha" >&2; exit @BESIDE_MISSING@; }
  fi
done
@BESIDE_ENV@=$beside; export @BESIDE_ENV@
"#;

const RUN: &str = r#"cd "$slot"
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; export PATH; fi
if [ -n "${runtime:-}" ]; then
  runtime_dir=$HOME/.local/state/north/runtimes/$runtime
  [ -x "$runtime_dir/north" ] || { echo "offload: the lander runtime $runtime is not on this host" >&2; exit @TRANSPORT_FAILED@; }
  PATH="$runtime_dir:$PATH"; export PATH
fi
@SLOT_ENV@=$slot; export @SLOT_ENV@
if [ -n "${carried:-}" ]; then eval "export $carried"; fi
if [ -n "${cap:-}" ]; then
  share=$((${cores:-1} / cap)); [ "$share" -ge 1 ] || share=1
  CARGO_BUILD_JOBS=$share RUST_TEST_THREADS=$share NEXTEST_TEST_THREADS=$share; export CARGO_BUILD_JOBS RUST_TEST_THREADS NEXTEST_TEST_THREADS
  case ${SAFE_PUSH_LAND_CPUS:-} in "" | *[!0-9]*) ;; *) [ "$SAFE_PUSH_LAND_CPUS" -le "$share" ] || SAFE_PUSH_LAND_CPUS=$share ;; esac
fi
wall=--wall
north capacity deadline --wall - 5 -- true 2>/dev/null || wall=
limited() {
  if command -v north >/dev/null 2>&1; then north capacity deadline ${wall:+"$wall"} - "$limit" -- "$@"; else timeout -k 5 "$limit" "$@"; fi
}
rc=0
if [ "${command#nix develop}" = "$command" ] && command -v north >/dev/null 2>&1; then
  limited north dev-shell run --dir "$slot" -- bash -euo pipefail -c "$command" 9>&- || rc=$?
else
  limited bash -euo pipefail -c "$command" 9>&- || rc=$?
fi
[ -z "${ran:-}" ] || printf "%s %s\n" "$ran" "$rc"
exit "$rc"
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
}

fn fail(why: &str) -> u8 {
    eprintln!("north capacity offload: {why}");
    TRANSPORT_FAILED
}

fn job_ref(head: &CommitId) -> String {
    static JOBS: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    format!("refs/land-offload/{}-{}-{nanos}-{}", head.as_str(), std::process::id(), JOBS.fetch_add(1, Ordering::Relaxed))
}

mod beside;
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

fn run_part() -> String {
    RUN.replace("@SLOT_ENV@", crate::capacity::land_claim::OFFLOAD_SLOT_ENV).replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
}

fn beside_part() -> String {
    BESIDE.replace("@BESIDE_MISSING@", &BESIDE_MISSING.to_string()).replace("@BESIDE_ENV@", beside::BESIDE_ENV)
}

fn slot_name(name: &str, index: u32) -> String {
    if index == 0 { name.to_owned() } else { format!("{name}-{index}") }
}

/// One block of `SLOTS` sccache ports per checkout, picked by FNV-1a of its name, so the build host's script needs no hashing program.
fn slot_ports(name: &str) -> Vec<(String, u32)> {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let block = u32::try_from(hash % u64::from(SCCACHE_PORTS / SLOTS)).unwrap_or(0) * SLOTS;
    (0..SLOTS).map(|index| (slot_name(name, index), SCCACHE_FIRST_PORT + block + index)).collect()
}

fn remote_script(name: &str) -> String {
    let ports: String = slot_ports(name).iter().map(|(slot, port)| format!("{}) SCCACHE_SERVER_PORT={port} ;; ", quote(slot))).collect();
    format!("{REMOTE}{VERIFY}{}{}", beside_part(), run_part())
        .replace("@SLOT_PORTS@", &ports)
        .replace("@ROOT@", OFFLOAD_ROOT)
        .replace("@LOCK_WAIT@", &LOCK_WAIT_SECONDS.to_string())
        .replace("@POLL@", POLL_SECONDS)
        .replace("@SLOTS@", &SLOTS.to_string())
        .replace("@TARGET_CAP@", &TARGET_CAP_MIB.to_string())
        .replace("@TARGET_FLOOR@", &TARGET_FLOOR_MIB.to_string())
        .replace("@SLOTS_BUSY@", &SLOTS_BUSY.to_string())
        .replace("@WAITED@", WAITED)
        .replace("@WRONG_TREE@", &WRONG_TREE.to_string())
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
}

pub(super) fn run(land: &Land) -> u8 {
    let (relative, name) = match checkout(land.checkout) {
        Ok(found) => found,
        Err(why) => return fail(&why),
    };
    let reference = job_ref(&land.head);
    let bundle = match bundle(land, &reference) {
        Ok(file) => file,
        Err(why) => return fail(&why),
    };
    let script = remote_script(&name);
    let own = crate::runtime::own_runtime();
    if let Some((sha, directory)) = &own
        && let Err(why) = ensure_shipped(land.ssh, sha, || crate::runtime::staged_on(land.ssh, sha), || crate::runtime::stage_on(land.ssh, sha, directory))
    {
        return fail(&why);
    }
    let runtime = own.map_or_else(String::new, |(sha, _)| sha);
    let base = land.base.as_ref().map_or("", CommitId::as_str);
    let owner = std::env::var(OWNER_ENV).unwrap_or_default();
    let cpus = (crate::capacity::host::land_check_cpus().ceil() as u32).to_string();
    let red_fix = std::env::var(RED_FIX_ENV).unwrap_or_default();
    let marker = ran_marker(&reference);
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let beside = beside::pin(land.command, &relative, &home, |checkout| CommitId::resolve(checkout, "HEAD").ok());
    let words = [relative.as_str(), base, land.head.as_str(), &land.timeout_seconds.to_string(), &name, &beside.command, &reference, &carried(land, |name| std::env::var(name).ok()), &owner, &runtime, &cpus, &red_fix, &marker, &beside.pins].map(quote);
    let remote = format!("setsid -w sh -c {} offload {}", quote(&script), words.join(" "));
    let spawned = with_caller("ssh")
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", land.ssh, "--", &remote])
        .stdin(Stdio::from(bundle))
        .stdout(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return fail(&format!("ssh: {error}")),
    };
    let ran = child.stdout.take().and_then(|stdout| relay(std::io::BufReader::new(stdout), &mut std::io::stdout(), &marker));
    let exited = match child.wait() {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()),
        Err(error) => return fail(&format!("ssh: {error}")),
    };
    verdict(exited, ran).unwrap_or_else(|why| fail(&why))
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

/// The run's exit code: the status the remote script reported for the command it ran, or ssh's own when that is a failure; a run whose remote never reported running its command fails, even when ssh exited 0.
fn verdict(exited: Option<u8>, ran: Option<u8>) -> Result<u8, String> {
    match (exited, ran) {
        (Some(0), Some(code)) => Ok(code),
        (Some(code), _) if code != 0 => Ok(code),
        (None, Some(code)) if code != 0 => Ok(code),
        _ => Err("the remote host never reported finishing the command, so the run fails".to_owned()),
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
