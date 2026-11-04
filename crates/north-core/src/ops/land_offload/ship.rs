use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) const TRANSPORT_FAILED: u8 = 255;
const LOCK_WAIT_SECONDS: u32 = 600;
const SLOTS: u32 = 6;
pub(super) const OFFLOAD_ROOT: &str = "${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}";

const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6
root=@ROOT@
ref=refs/land-offload/$head
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
  git -C "$repo" fetch -q origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q "$bundle" "+$ref:$ref"
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e /target
"#;

const RUN: &str = r#"cd "$slot"
export SAFE_PUSH_LAND_BASE=$base SAFE_PUSH_LAND_HEAD=$head
if [ -e "$slot/flake.nix" ] && [ "${command#nix develop}" = "$command" ]; then
  timeout -k 5 "$limit" nix develop "$slot" -c bash -euo pipefail -c "$command" 9>&-
else
  timeout -k 5 "$limit" bash -euo pipefail -c "$command" 9>&-
fi
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
    let script = format!("{REMOTE}{RUN}").replace("@ROOT@", OFFLOAD_ROOT).replace("@LOCK_WAIT@", &LOCK_WAIT_SECONDS.to_string()).replace("@SLOTS@", &SLOTS.to_string());
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

#[cfg(test)]
mod tests {
    use super::RUN;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::Command;

    fn remote_run(command: &str, flake: bool) -> String {
        let slot = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        if flake {
            fs::write(slot.path().join("flake.nix"), "{}").unwrap();
        }
        let nix = bin.path().join("nix");
        fs::write(&nix, "#!/bin/sh\necho \"nix $*\"\n").unwrap();
        fs::set_permissions(&nix, fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap());
        let output = Command::new("sh")
            .args(["-c", &format!("set -eu\nslot={} command={} limit=10 base=b head=h\n{RUN}", shell(slot.path()), shell(Path::new(command)))])
            .env("PATH", path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().replace(&slot.path().display().to_string(), "SLOT")
    }

    fn shell(word: &Path) -> String {
        super::quote(&word.display().to_string())
    }

    #[test]
    fn a_checkout_with_a_flake_runs_the_command_inside_its_dev_shell() {
        assert_eq!(remote_run("cargo --version", true), "nix develop SLOT -c bash -euo pipefail -c cargo --version\n");
    }

    #[test]
    fn a_checkout_without_a_flake_runs_the_command_plainly() {
        assert_eq!(remote_run("echo plain", false), "plain\n");
    }

    #[test]
    fn a_command_that_already_enters_a_dev_shell_is_not_wrapped_again() {
        assert_eq!(remote_run("nix develop -c cargo --version", true), "nix develop -c cargo --version\n");
    }
}
