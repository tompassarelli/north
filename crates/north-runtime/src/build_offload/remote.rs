use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::lane::Lane;
use crate::capacity::build_offload::{Declined, Job, REMOTE_MARK, SHIP_SECONDS_ENV, SLOT_HEAD_ENV, SLOT_TARGET_ENV};
use crate::capacity::handover::{Frame, Offer};
use crate::land_offload::ship::quote;

const SYNC_LIMIT: Duration = Duration::from_secs(180);
pub(super) const SYNCED: &str = "NORTH-OFFLOAD-SYNCED";

const SCRIPT: &str = r#"set -eu
repo=$HOME/$1 head=$2 name=$3 blen=$4 plen=$5 sub=$6 owner=$7 limit=$8
shift 8
root=$HOME/.cache/build-offload
slot=$root/$name
north=${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}
mkdir -p "$root"
exec 9>"$root/$name.lock"
flock -w 20 9 || { echo "offload: the $name lane is busy" >&2; exit 96; }
began=$(date +%s.%N)
export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=gc.auto GIT_CONFIG_VALUE_0=0
trap 'rm -f "$root/$name.bundle" "$root/$name.patch"' EXIT
slot_target=0 slot_head=
[ -d "$slot/target" ] && slot_target=1
slot_head=$(git -C "$slot" rev-parse HEAD 2>/dev/null || true)
head -c "$blen" >"$root/$name.bundle"
head -c "$plen" >"$root/$name.patch"
bundled() { [ "$blen" -gt 0 ] && git -C "$repo" fetch -q --no-auto-maintenance "$root/$name.bundle" "+HEAD:refs/build-offload/$name"; }
bundled || { git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true; bundled || true; }
git -C "$repo" cat-file -e "$head^{commit}" 2>/dev/null || { echo "offload: $repo lacks $head" >&2; exit 97; }
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree prune; git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e target -e node_modules
if [ "$plen" -gt 0 ]; then git -C "$slot" apply --binary --whitespace=nowarn "$root/$name.patch"; fi
shipped=$(awk -v a="$began" -v b="$(date +%s.%N)" 'BEGIN { printf "%.2f", b - a }')
cd "$slot/$sub"
if [ "${1##*/}" = north ] && [ "${1#/}" != "$1" ] && [ ! -x "$1" ]; then shift; set -- "$north" "$@"; fi
echo @SYNCED@
exec 8<&0
@MARK@=1 @SHIP@=$shipped @TARGET@=$slot_target @HEAD@=$slot_head "$north" capacity host run --class heavy --owner "$owner" --timeout-seconds "$limit" -- "$@" 9>&- 8<&- &
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

/// The run a lane and job make as a peer is offered it.
pub(super) fn offer(lane: &Lane, job: &Job) -> Offer {
    Offer {
        repo: lane.repo.clone(),
        head: lane.head.clone(),
        name: lane.name.clone(),
        subdirectory: lane.subdirectory.clone(),
        owner: job.owner.to_owned(),
        timeout_seconds: job.timeout_seconds,
        command: job.command.to_vec(),
        bundle_bytes: lane.bundle.len() as u64,
        patch_bytes: lane.patch.len() as u64,
    }
}

/// The script that rebuilds a lane on a host and runs the job there under that host's capacity run, which enters the lane's flake dev shell.
pub(super) fn script() -> String {
    SCRIPT.replace("@SYNCED@", SYNCED).replace("@MARK@", REMOTE_MARK).replace("@SHIP@", SHIP_SECONDS_ENV).replace("@TARGET@", SLOT_TARGET_ENV).replace("@HEAD@", SLOT_HEAD_ENV)
}

/// The arguments the script takes for an offer.
pub(super) fn words(offer: &Offer) -> Vec<String> {
    let mut words = vec![offer.repo.clone(), offer.head.clone(), offer.name.clone(), offer.bundle_bytes.to_string(), offer.patch_bytes.to_string(), offer.subdirectory.clone(), offer.owner.clone(), offer.timeout_seconds.to_string()];
    words.extend(offer.command.iter().cloned());
    words
}

pub(super) fn invocation(lane: &Lane, job: &Job) -> (String, Vec<String>) {
    (script(), words(&offer(lane, job)))
}

/// Rebuilds the lane on the host, runs the command there under that host's own heavy admission, streams its output here and returns its exit code. A failure before the command starts is `NotSynced`.
pub(super) fn run(ssh: &str, lane: &Lane, job: &Job, sink: Option<File>, started: &dyn Fn()) -> Ran {
    let (script, words) = invocation(lane, job);
    let quoted: Vec<String> = words.iter().map(|word| quote(word)).collect();
    let remote = format!("sh -c {} offload {}", quote(&script), quoted.join(" "));
    let mut ssh_command = Command::new("ssh");
    ssh_command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ServerAliveInterval=30", "-o", "ServerAliveCountMax=4", ssh, "--", &remote]);
    drive(ssh_command, lane, sink, started)
}

pub(super) fn drive(mut transport: Command, lane: &Lane, sink: Option<File>, started: &dyn Fn()) -> Ran {
    let spawned = transport
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
    let mut output: Box<dyn Write> = sink.map_or_else(|| Box::new(std::io::stdout()) as Box<dyn Write>, |file| Box::new(file));
    let _ = std::io::copy(&mut reader, &mut output);
    let _ = output.flush();
    drop(stdin);
    let status = child.wait();
    Ran::Exit(status.ok().and_then(|status| status.code()).and_then(|code| u8::try_from(code).ok()).unwrap_or(255))
}


const OFFER_REPLY_LIMIT: Duration = Duration::from_secs(SYNC_LIMIT.as_secs() + 30);

/// Offers the lane to the peer that opened `peer`, shows what the peer sends back and returns the exit code it reports; a peer that cannot rebuild the lane, or leaves before the command starts, leaves the run queued.
pub(super) fn hand_over(mut peer: UnixStream, lane: &Lane, job: &Job, sink: Option<File>, started: &dyn Fn(&str)) -> Result<u8, Option<Declined>> {
    let gone = |error: std::io::Error| Some(Declined::Failed(format!("the taking host left: {error}")));
    peer.set_read_timeout(Some(OFFER_REPLY_LIMIT)).map_err(gone)?;
    let sent = peer.write_all(offer(lane, job).to_line().as_bytes()).and_then(|()| peer.write_all(&lane.bundle)).and_then(|()| peer.write_all(&lane.patch)).and_then(|()| peer.flush());
    sent.map_err(gone)?;
    let mut output: Box<dyn Write> = sink.map_or_else(|| Box::new(std::io::stdout()) as Box<dyn Write>, |file| Box::new(file));
    let mut running = false;
    loop {
        match Frame::read(&mut peer) {
            Ok(Some(Frame::Started(host))) => {
                running = true;
                let _ = peer.set_read_timeout(None);
                started(&host);
            }
            Ok(Some(Frame::Out(bytes))) => {
                let _ = output.write_all(&bytes).and_then(|()| output.flush());
            }
            Ok(Some(Frame::Err(bytes))) => {
                let _ = std::io::stderr().write_all(&bytes);
            }
            Ok(Some(Frame::Exit(code))) => return Ok(code),
            Ok(Some(Frame::NotSynced(why))) => return Err(Some(Declined::Failed(why))),
            Ok(None) | Err(_) if running => return Ok(255),
            Ok(None) => return Err(Some(Declined::Failed("the taking host ended the connection before the lane synced".to_owned()))),
            Err(error) => return Err(Some(Declined::Failed(format!("the taking host: {error}")))),
        }
    }
}
