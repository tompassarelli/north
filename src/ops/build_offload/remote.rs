use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::lane::Lane;
use crate::capacity::build_offload::{Job, REMOTE_MARK};
use crate::ops::land_offload::ship::quote;

const SYNC_LIMIT: Duration = Duration::from_secs(180);
const SYNCED: &str = "NORTH-OFFLOAD-SYNCED";

const SCRIPT: &str = r#"set -eu
repo=$HOME/$1 head=$2 name=$3 blen=$4 plen=$5 sub=$6 owner=$7 limit=$8
shift 8
root=$HOME/.cache/build-offload
slot=$root/$name
north=${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}
mkdir -p "$root"
exec 9>"$root/$name.lock"
flock -w 20 9 || { echo "offload: the $name lane is busy" >&2; exit 96; }
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=gc.auto GIT_CONFIG_VALUE_0=0
trap 'rm -f "$root/$name.bundle" "$root/$name.patch"' EXIT
head -c "$blen" >"$root/$name.bundle"
head -c "$plen" >"$root/$name.patch"
bundled() { [ "$blen" -gt 0 ] && git -C "$repo" fetch -q "$root/$name.bundle" "+HEAD:refs/build-offload/$name"; }
bundled || { git -C "$repo" fetch -q origin 2>/dev/null || true; bundled || true; }
git -C "$repo" cat-file -e "$head^{commit}" 2>/dev/null || { echo "offload: $repo lacks $head" >&2; exit 97; }
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e target -e node_modules
if [ "$plen" -gt 0 ]; then git -C "$slot" apply --binary --whitespace=nowarn "$root/$name.patch"; fi
cd "$slot/$sub"
if [ -e "$slot/flake.nix" ] && [ "${1##*/}" != nix ]; then set -- nix develop "$slot" -c "$@"; fi
echo @SYNCED@
exec 8<&0
@MARK@=1 "$north" capacity host run --class heavy --owner "$owner" --timeout-seconds "$limit" -- "$@" 9>&- 8<&- &
child=$!
( cat <&8 >/dev/null; kill -TERM "$child" 2>/dev/null ) 9>&- &
watcher=$!
trap 'kill -TERM "$child" 2>/dev/null' HUP INT TERM
code=0
while :; do wait "$child" && code=0 || code=$?; kill -0 "$child" 2>/dev/null || break; done
kill "$watcher" 2>/dev/null || true
exit "$code"
"#;

pub(super) enum Ran {
    Exit(u8),
    NotSynced(String),
}

/// The script and its arguments that rebuild the lane on the host and run the job there, inside the lane's flake devshell when it has one, since a host may keep its toolchains only in project devshells.
fn invocation(lane: &Lane, job: &Job) -> (String, Vec<String>) {
    let script = SCRIPT.replace("@SYNCED@", SYNCED).replace("@MARK@", REMOTE_MARK);
    let mut words = vec![lane.repo.clone(), lane.head.clone(), lane.name.clone(), lane.bundle.len().to_string(), lane.patch.len().to_string(), lane.subdirectory.clone(), job.owner.to_owned(), job.timeout_seconds.to_string()];
    words.extend(job.command.iter().cloned());
    (script, words)
}

/// Rebuilds the lane on the host, runs the command there under that host's own heavy admission, streams its output here and returns its exit code. A failure before the command starts is `NotSynced`.
pub(super) fn run(ssh: &str, lane: &Lane, job: &Job, started: &dyn Fn()) -> Ran {
    let (script, words) = invocation(lane, job);
    let quoted: Vec<String> = words.iter().map(|word| quote(word)).collect();
    let remote = format!("sh -c {} offload {}", quote(&script), quoted.join(" "));
    let spawned = Command::new("ssh")
        .args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", ssh, "--", &remote])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return Ran::NotSynced(format!("ssh: {error}")),
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return Ran::NotSynced("ssh gave no pipes".to_owned()) };
    let synced = std::thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        scope.spawn(move || {
            let sent = stdin.write_all(&lane.bundle).and_then(|()| stdin.write_all(&lane.patch)).and_then(|()| stdin.flush());
            let mut reader = BufReader::new(stdout);
            let mut first = String::new();
            let ready = sent.is_ok() && reader.read_line(&mut first).is_ok_and(|read| read > 0) && first.trim_end() == SYNCED;
            let _ = sender.send(ready.then_some((stdin, reader)));
        });
        let ready = receiver.recv_timeout(SYNC_LIMIT).ok().flatten();
        if ready.is_none() {
            let _ = child.kill();
        }
        ready
    });
    let Some((stdin, mut reader)) = synced else {
        let _ = child.kill();
        let _ = child.wait();
        return Ran::NotSynced("the host could not rebuild the lane in time".to_owned());
    };
    started();
    let _ = std::io::copy(&mut reader, &mut std::io::stdout());
    drop(stdin);
    let status = child.wait();
    Ran::Exit(status.ok().and_then(|status| status.code()).and_then(|code| u8::try_from(code).ok()).unwrap_or(255))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn executable(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn git(directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git").current_dir(directory).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(arguments).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn ran_on_host(files: &[&str]) -> String {
        let home = tempfile::tempdir().unwrap();
        let tools = home.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        executable(&tools.join("north"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n");
        executable(&tools.join("nix"), "#!/bin/sh\necho \"nix $*\"\n");
        executable(&tools.join("cargo"), "#!/bin/sh\necho \"cargo $*\"\n");
        let repo = home.path().join("code/demo/main");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        for file in files {
            std::fs::write(repo.join(file), "").unwrap();
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "lane"]);
        let lane = Lane { repo: "code/demo/main".to_owned(), name: "code-demo-main-lane".to_owned(), head: git(&repo, &["rev-parse", "HEAD"]), bundle: Vec::new(), patch: Vec::new(), subdirectory: String::new() };
        let command = ["cargo".to_owned(), "test".to_owned()];
        let (script, words) = invocation(&lane, &Job { owner: "test", timeout_seconds: 60, command: &command });
        let path = format!("{}:{}", tools.display(), std::env::var("PATH").unwrap_or_default());
        let mut child = Command::new("sh").arg("-c").arg(&script).arg("offload").args(&words).env("HOME", home.path()).env("PATH", path).env("AGENT_CAPACITY_NORTH", tools.join("north")).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut lines = [String::new(), String::new()];
        for line in &mut lines {
            reader.read_line(line).unwrap();
        }
        drop(child.stdin.take());
        child.wait().unwrap();
        assert_eq!(lines[0].trim_end(), SYNCED);
        lines[1].trim_end().replace(&home.path().join(".cache/build-offload").display().to_string(), "SLOTS")
    }

    #[test]
    fn capacity_an_offloaded_command_runs_inside_the_lanes_flake_devshell_when_it_has_one() {
        assert_eq!(ran_on_host(&["flake.nix"]), "nix develop SLOTS/code-demo-main-lane -c cargo test");
        assert_eq!(ran_on_host(&["README"]), "cargo test");
    }
}
