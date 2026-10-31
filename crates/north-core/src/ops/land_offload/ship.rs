use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) const TRANSPORT_FAILED: u8 = 255;
const LOCK_WAIT_SECONDS: u32 = 600;
pub(super) const OFFLOAD_ROOT: &str = "${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}";

const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6
root=@ROOT@
ref=refs/land-offload/$head
bundle=$root/$name.bundle
slot=$root/$name
mkdir -p "$root"
exec 9>"$root/$name.lock"
flock -w @LOCK_WAIT@ 9 || { echo "offload: another landing has held the $name slot for @LOCK_WAIT@ s" >&2; exit 96; }
cleanup() {
  rm -f "$bundle"
  git -C "$repo" update-ref -d "$ref" 2>/dev/null || true
  if [ -e "$slot/.git" ]; then git -C "$slot" clean -ffdxq -e /target 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 143' HUP INT TERM
cat >"$bundle"
if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
  git -C "$repo" fetch -q origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q "$bundle" "+$ref:$ref"
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e /target
cd "$slot"
SAFE_PUSH_LAND_BASE=$base SAFE_PUSH_LAND_HEAD=$head timeout -k 5 "$limit" bash -euo pipefail -c "$command" 9>&-
"#;

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
    pub(super) base: &'a str,
    pub(super) head: &'a str,
    pub(super) timeout_seconds: u64,
    pub(super) command: &'a str,
}

fn fail(why: &str) -> u8 {
    eprintln!("north capacity offload: {why}");
    TRANSPORT_FAILED
}

fn bundle(land: &Land) -> Result<File, String> {
    let reference = format!("refs/land-offload/{}", land.head);
    let file = tempfile::tempfile().map_err(|error| format!("bundle file: {error}"))?;
    let made = Command::new("git").args(["update-ref", &reference, land.head]).status().is_ok_and(|status| status.success());
    if !made {
        return Err(format!("cannot name {} for the bundle", land.head));
    }
    let mut command = Command::new("git");
    command.args(["bundle", "create", "-q", "-", &reference]);
    if !land.base.is_empty() {
        command.arg(format!("^{}", land.base));
    }
    let bundled = command.stdout(file.try_clone().map_err(|error| error.to_string())?).status().is_ok_and(|status| status.success());
    let _ = Command::new("git").args(["update-ref", "-d", &reference]).status();
    if !bundled {
        return Err("git bundle failed".to_owned());
    }
    let mut file = file;
    file.rewind().map_err(|error| error.to_string())?;
    Ok(file)
}

pub(super) fn run(land: &Land) -> u8 {
    let (relative, name) = match checkout(Path::new(".")) {
        Ok(found) => found,
        Err(why) => return fail(&why),
    };
    let bundle = match bundle(land) {
        Ok(file) => file,
        Err(why) => return fail(&why),
    };
    let script = REMOTE.replace("@ROOT@", OFFLOAD_ROOT).replace("@LOCK_WAIT@", &LOCK_WAIT_SECONDS.to_string());
    let words = [relative.as_str(), land.base, land.head, &land.timeout_seconds.to_string(), &name, land.command].map(quote);
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
