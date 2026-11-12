//! The user service a session another host placed on this machine runs as: the script that holds
//! its CLI, the one that prunes what gone services left, the unit's name, and the work below its
//! CLI, read over ssh since the placing host's procfs holds only the link.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::platform::process::try_run_command_within;
use sha2::{Digest, Sha256};

/// `sh -c HOLD DIR UNIT DETACHED_SECS CLI...`: the service's main process. It feeds the CLI the
/// input file through a FIFO, records the CLI's status and stops the service once no link has been
/// attached for DETACHED_SECS.
pub const HOLD: &str = r#"d=$0 u=$1 limit=$2; shift 2
mkfifo "$d/feed" || exit 70
{ "$@" < "$d/feed" >> "$d/out" 2>> "$d/err"; echo $? > "$d/status"; } &
cli=$!
{ seen=$(date +%s); while sleep 60; do
  if read -r p b < "$d/attach" && [ "$(cut -d' ' -f22 "/proc/$p/stat")" = "$b" ]; then seen=$(date +%s)
  elif [ $(($(date +%s) - seen)) -ge "$limit" ]; then systemctl --user stop "$u"; fi
done; } 2>/dev/null &
watch=$!
tail -c +1 -f --pid="$cli" "$d/in" > "$d/feed"
wait "$cli"; code=$?
kill "$watch" 2>/dev/null
exit "$code""#;

/// `sh -c PRUNE HELD UNIT`: removes held directories untouched for three days whose service is gone, other than UNIT's, which a launch beside it is making.
pub const PRUNE: &str = r#"for h in "$0"/*.service; do
  [ "${h##*/}" != "$1" ] && [ -d "$h" ] && [ -z "$(find "$h" -maxdepth 1 -mmin -4320 -print -quit)" ] && ! systemctl --user -q is-active "${h##*/}" &&
    rm -f "$h/in" "$h/out" "$h/err" "$h/status" "$h/attach" "$h/feed" "$h/nonce" && rmdir "$h"
done"#;

/// The session's unit, named for reading and suffixed with a digest of the launching host and the
/// exact session name, so neither sanitised look-alikes nor another launching host share it.
pub fn unit(key: &str) -> String {
    let name: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let launcher = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let digest = Sha256::digest(format!("{}\0{key}", launcher.trim()).as_bytes());
    let suffix: String = digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect();
    format!("north-fleet-{name}-{suffix}.service")
}

/// `sh -c WORK north-work UNIT...`: prints `COUNT UNIT` for each held unit, COUNT being its live
/// processes at least three below its main process, which runs the CLI in a subshell.
const WORK: &str = r#"for u; do
  m=$(systemctl --user show -p MainPID --value "$u" 2>/dev/null)
  ps -e -o pid= -o ppid= -o stat= | awk -v m="${m:-0}" -v u="$u" '$3 !~ /^Z/ { up[$1] = $2 } END { n = 0; for (p in up) { d = 0; q = p; while (q in up && q != m && d < 64) { q = up[q]; d++ } if (m > 0 && q == m && d >= 3) n++ } print n, u }'
done"#;
const WAIT: Duration = Duration::from_secs(8);

/// How many processes run below the CLI of each held unit in `units` on the machine `destination`
/// names, by unit; `None` when the machine does not answer for every unit, so unseen work is not
/// taken for none.
pub fn work(ssh: &Path, destination: &str, units: &[String]) -> Option<BTreeMap<String, usize>> {
    let quote = |word: &str| format!("'{}'", word.replace('\'', r"'\''"));
    let script = format!("sh -c {} north-work {}", quote(WORK), units.iter().map(|unit| quote(unit)).collect::<Vec<_>>().join(" "));
    let mut command = Command::new(ssh);
    command.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "--", &script]);
    let output = try_run_command_within(command, None, WAIT).ok().flatten().filter(|output| output.status.success())?;
    let counted: BTreeMap<String, usize> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(' ').and_then(|(count, unit)| Some((unit.to_owned(), count.parse().ok()?))))
        .filter(|(unit, _)| units.contains(unit))
        .collect();
    (counted.len() == units.len()).then_some(counted)
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// `sh -c FREE KEPT MACHINE LANE SSH ARGS...`: runs the removal over ssh and ends normally when `north
/// lane remove` keeps the lane (its exit 1), writing `SECONDS<TAB>MACHINE<TAB>LANE<TAB>REASON` to KEPT and
/// to its own output; any other end, ssh's 255 included, repeats the output on stderr and exits with it.
const FREE: &str = r#"log=$0 machine=$1 lane=$2; shift 2
out=$("$@" 2>&1); code=$?
[ "$code" -eq 1 ] || { printf '%s\n' "$out" >&2; exit "$code"; }
reason=$(printf %s "${out#"kept $lane: "}" | tr '\t\n' '  ')
printf '%s\t%s\t%s\t%s\n' "$(date +%s)" "$machine" "$lane" "$reason" >> "$log"
printf 'lane %s on %s kept: %s\n' "$lane" "$machine" "$reason""#;

/// A lane `north lane remove` kept after its session ended, as the free step recorded it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeptLane {
    pub at_secs: u64,
    pub machine: String,
    pub lane: String,
    pub reason: String,
}

/// The lanes recorded in `kept` at or after `since_secs`, oldest first.
pub fn kept_lanes(kept: &Path, since_secs: u64) -> Vec<KeptLane> {
    let text = std::fs::read_to_string(kept).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\t');
            let at_secs = fields.next()?.parse().ok()?;
            Some(KeptLane { at_secs, machine: fields.next()?.to_owned(), lane: fields.next()?.to_owned(), reason: fields.next()?.to_owned() })
        })
        .filter(|lane| lane.at_secs >= since_secs)
        .collect()
}

fn free_command(ssh: &Path, destination: &str, machine: &str, lane: &Path, kept: &Path) -> Command {
    let script = format!("PATH=\"$HOME/.local/bin:$PATH\" north lane remove {}", quote(&lane.to_string_lossy()));
    let mut command = Command::new("sh");
    command.args(["-c", FREE]).arg(kept).arg(machine).arg(lane).arg(ssh).args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "--", &script]);
    command.stdin(Stdio::null());
    command
}

/// Frees the lane a session worked in on the machine it ran on, once the session has ended: runs
/// `north lane remove` there, which removes a finished lane with its worktree and target and keeps
/// one that still holds unlanded, uncommitted, locked or queued work; a kept lane is recorded in
/// `kept` and is not a failure. A deletion outlives the fleet tick that asked for it: the ssh runs as
/// its own transient user unit, so the tick's cgroup stop does not kill it, and a host without a user
/// manager falls back to a plain child.
pub fn free_lane(ssh: &Path, destination: &str, machine: &str, lane: &Path, kept: &Path) -> Result<(), String> {
    let free = free_command(ssh, destination, machine, lane, kept);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let unit = format!("north-lane-free-{}-{stamp}", std::process::id());
    let mut detached = Command::new("systemd-run");
    detached.args(["--user", "--quiet", "--collect", "--unit", &unit]);
    if let Some(socket) = std::env::var("SSH_AUTH_SOCK").ok().filter(|socket| !socket.is_empty()) {
        detached.arg(format!("--setenv=SSH_AUTH_SOCK={socket}"));
    }
    let started = detached.arg("--").arg(free.get_program()).args(free.get_args()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    if started.is_ok_and(|status| status.success()) {
        return Ok(());
    }
    let mut fallback = free;
    fallback.stdout(Stdio::null()).stderr(Stdio::null());
    fallback.spawn().map(drop).map_err(|error| format!("cannot free lane {} on {destination}: {error}", lane.display()))
}

/// A held session's service running on this machine: when it became active and whether a placing
/// host's link is attached to it now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeldUnit {
    pub unit: String,
    pub active_since_ms: u64,
    pub attached: bool,
}

/// The session a held unit runs, read back from its name: `north-fleet-NAME-SUFFIX.service`.
pub fn session_of(unit: &str) -> Option<&str> {
    let (name, suffix) = unit.strip_prefix("north-fleet-")?.strip_suffix(".service")?.rsplit_once('-')?;
    (suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) && !name.is_empty()).then_some(name)
}

/// The held units active on this machine, each with its link read from `held/UNIT/attach`; empty
/// when systemd does not answer, so a silent systemd never reads as units to stop.
pub fn active_units(held: &Path) -> Vec<HeldUnit> {
    let listed = systemctl(&["list-units", "--type=service", "--state=active", "--plain", "--no-legend", "north-fleet-*"]);
    let units: Vec<String> = listed.lines().filter_map(|line| line.split_whitespace().next()).filter(|unit| session_of(unit).is_some()).map(str::to_owned).collect();
    if units.is_empty() {
        return Vec::new();
    }
    let mut show = vec!["show", "-p", "Id", "-p", "ActiveEnterTimestamp", "--timestamp=unix"];
    show.extend(units.iter().map(String::as_str));
    systemctl(&show)
        .split("\n\n")
        .filter_map(|block| {
            let value = |key: &str| block.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='));
            let unit = value("Id")?.to_owned();
            let active_since_ms = value("ActiveEnterTimestamp")?.strip_prefix('@')?.parse::<u64>().ok()? * 1000;
            let attached = attached(&held.join(&unit).join("attach"));
            units.contains(&unit).then_some(HeldUnit { unit, active_since_ms, attached })
        })
        .collect()
}

/// Whether the process an attach file names (`PID START`) still runs with that start time.
fn attached(attach: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(attach) else { return false };
    let mut words = text.split_whitespace();
    let (Some(pid), Some(start)) = (words.next(), words.next()) else { return false };
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().nth(19)) == Some(start))
}

/// Stops a held unit without waiting for its CLI to exit.
pub fn stop(unit: &str) -> Result<(), String> {
    let mut command = Command::new("systemctl");
    command.args(["--user", "stop", "--no-block", unit]);
    match try_run_command_within(command, None, WAIT) {
        Ok(Some(output)) if output.status.success() => Ok(()),
        Ok(Some(output)) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Ok(None) => Err(format!("systemctl did not answer within {}s", WAIT.as_secs())),
        Err(error) => Err(error.to_string()),
    }
}

fn systemctl(arguments: &[&str]) -> String {
    let mut command = Command::new("systemctl");
    command.arg("--user").args(arguments);
    try_run_command_within(command, None, WAIT).ok().flatten().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default()
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
