//! The user service a session another host placed on this machine runs as: the script that holds
//! its CLI, the one that prunes what gone services left, the unit's name, and the work below its
//! CLI, read over ssh since the placing host's procfs holds only the link.
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
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

/// `sh -c PRUNE HELD`: removes held directories untouched for three days whose service is gone.
pub const PRUNE: &str = r#"for h in "$0"/*.service; do
  [ -d "$h" ] && [ -z "$(find "$h" -maxdepth 1 -mmin -4320 -print -quit)" ] && ! systemctl --user -q is-active "${h##*/}" &&
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
