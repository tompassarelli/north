use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const TRANSPORT_FAILED: u8 = 255;
const WRONG_TREE: u8 = 98;
const LOCK_WAIT_SECONDS: u32 = 600;
const SLOTS: u32 = 6;
pub(super) const OFFLOAD_ROOT: &str = "${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}";

const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6 ref=$7 landing=${8:-} landref=${9:-}
root=@ROOT@
mkdir -p "$root"
waited=0
while :; do
  i=0
  while [ "$i" -lt @SLOTS@ ]; do
    s=$name
    [ "$i" -eq 0 ] || s=$name-$i
    exec 9>"$root/$s.lock"
    if flock -n 9; then break 2; fi
    exec 9>&-
    i=$((i + 1))
  done
  [ "$waited" -lt @LOCK_WAIT@ ] || { echo "offload: all @SLOTS@ $name slots stayed busy for @LOCK_WAIT@ s" >&2; exit 96; }
  [ $((waited % 60)) -ne 0 ] || echo "offload: all @SLOTS@ $name slots busy for ${waited}s, waiting up to @LOCK_WAIT@ s; held by: $(cat "$root/$name".holder "$root/$name"-*.holder 2>/dev/null | tr '\n' ';')" >&2
  sleep 2
  waited=$((waited + 2))
done
bundle=$root/$s.bundle
slot=$root/$s
printf '%s since %s\n' "$(printf '%.12s' "$head")" "$(date +%H:%M:%S)" >"$root/$s.holder"
cleanup() {
  rm -f "$bundle"
  git -C "$repo" update-ref -d "$ref" 2>/dev/null || true
  if [ -e "$slot/.git" ]; then git -C "$slot" clean -ffdxq -e /target 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 143' HUP INT TERM
cat >"$bundle"
if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
  git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q --no-auto-maintenance "$bundle" "+$ref:$ref"
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e /target
"#;

const VERIFY: &str = r#"landed=$(git -C "$slot" rev-parse HEAD)
[ "$landed" = "$head" ] || { echo "offload: refusing to run: $slot is on $landed, not the shipped $head" >&2; exit @WRONG_TREE@; }
echo "offload: running $head in $slot"
"#;

const RUN: &str = r#"cd "$slot"
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; export PATH; fi
if [ "$landing" = 1 ]; then
  export SAFE_PUSH_LAND_BASE=$base SAFE_PUSH_LAND_HEAD=$head SAFE_PUSH_LAND_REF=$landref SAFE_PUSH_LANDING=1
fi
limited() {
  if command -v north >/dev/null 2>&1; then north capacity deadline "" "$limit" -- "$@"; else timeout -k 5 "$limit" "$@"; fi
}
if [ -e "$slot/flake.nix" ] && [ "${command#nix develop}" = "$command" ]; then
  limited nix develop "$slot" -c bash -euo pipefail -c "$command" 9>&-
else
  limited bash -euo pipefail -c "$command" 9>&-
fi
"#;

/// ```compile_fail
/// let _: north_core::ops::land_offload::ship::CommitId = "HEAD".into();
/// ```
///
/// ```no_run
/// let _ = north_core::ops::land_offload::ship::CommitId::resolve(std::path::Path::new("/"), "HEAD");
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
    let relative = main.strip_prefix(&home).map_err(|_| format!("{} is not under {}, so the remote host cannot be assumed to keep it", main.display(), home.display()))?;
    let relative = relative.to_string_lossy().into_owned();
    Ok((relative.clone(), relative.replace('/', "-")))
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
    if let Some(base) = &land.base {
        command.arg(format!("^{}", base.as_str()));
    }
    let bundled = command.stdout(file.try_clone().map_err(|error| error.to_string())?).status().is_ok_and(|status| status.success());
    let _ = git().args(["update-ref", "-d", reference]).status();
    if !bundled {
        return Err("git bundle failed".to_owned());
    }
    let mut file = file;
    file.rewind().map_err(|error| error.to_string())?;
    Ok(file)
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
    let script = format!("{REMOTE}{VERIFY}{RUN}")
        .replace("@ROOT@", OFFLOAD_ROOT)
        .replace("@LOCK_WAIT@", &LOCK_WAIT_SECONDS.to_string())
        .replace("@SLOTS@", &SLOTS.to_string())
        .replace("@WRONG_TREE@", &WRONG_TREE.to_string());
    let base = land.base.as_ref().map_or("", CommitId::as_str);
    let landing = if std::env::var("SAFE_PUSH_LANDING").is_ok_and(|value| value == "1") { "1" } else { "" };
    let landref = if landing == "1" { std::env::var("SAFE_PUSH_LAND_REF").unwrap_or_default() } else { String::new() };
    let words = [relative.as_str(), base, land.head.as_str(), &land.timeout_seconds.to_string(), &name, land.command, &reference, landing, &landref].map(quote);
    let remote = format!("sh -c {} offload {}", quote(&script), words.join(" "));
    let sent = Command::new("ssh")
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", land.ssh, "--", &remote])
        .stdin(Stdio::from(bundle))
        .status();
    match sent {
        Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(TRANSPORT_FAILED),
        Err(error) => fail(&format!("ssh: {error}")),
    }
}

#[cfg(test)]
mod tests;
