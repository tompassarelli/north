//! The script the doctor runs on each host it probes, and the facts it reads back: free disk, held slots, prunable worktree registrations and stray land queues.

use std::path::{Path, PathBuf};

use super::SlotHold;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub free_percent: f64,
    pub free_gib: f64,
    pub slots: Vec<SlotHold>,
    pub prunable: Vec<(PathBuf, usize)>,
    /// Land queues under the probe's state home that hold no ticket in any area and that no lander holds.
    pub orphaned: Vec<PathBuf>,
}

const PROBE: &str = r#"set -u
@SLOT_HELD@root=${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}
now=$(date +%s)
df -Pk "$HOME" | awk 'NR == 2 { print "disk", $3, $4 }'
for h in "$root"/*.holder; do
  [ -f "$h" ] || continue
  base=${h%.holder}
  case $base in *.background) continue ;; esac
  if slot_held "$base.lock"; then state=locked; else state=free; fi
  pgid=$(cat "$base.pgid" 2>/dev/null || true)
  echo "slot ${base##*/} $((now - $(stat -c %Y "$h"))) ${pgid:--} $state"
done
for g in "$HOME"/code/*/main/.git; do
  [ -d "$g" ] || continue
  n=$(git -C "${g%/.git}" worktree list --porcelain 2>/dev/null | grep -c '^prunable' || true)
  [ "$n" = 0 ] || echo "prunable $n ${g%/.git}"
done
@ORPHANS@
"#;

/// The probe script that reads a host's disk, held slots, prunable worktree entries and stray land queues.
pub fn probe() -> String {
    PROBE.replace("@SLOT_HELD@", north_base::foundation::offload::SLOT_HELD_SH).replace("@ORPHANS@", &orphan_scan(None))
}

/// The script that names each land queue directly under `root` (the host's land-queue when `None`) that holds no ticket in any area, no lander lock and no file touched in the last hour, so a queue a test or a stray write left on an offload host is found and removed while a live outbox is left alone.
pub fn orphan_scan(root: Option<&Path>) -> String {
    let root = root.map_or_else(|| "\"${XDG_STATE_HOME:-$HOME/.local/state}/north/land-queue\"".to_owned(), |root| format!("\"{}\"", root.display()));
    format!(
        r#"root={root}
if [ -d "$root" ]; then
  for q in "$root"/*; do
    [ -d "$q" ] && [ ! -L "$q" ] || continue
    name=${{q##*/}}
    [ "${{#name}}" = 16 ] || continue
    case $name in *[!0-9a-f]*) continue ;; esac
    [ -f "$q/lander.lock" ] && continue
    find "$q" -type f -name '*.json' -print -quit | grep -q . && continue
    [ -n "$(find "$q" -type f -mmin -60 -print -quit 2>/dev/null)" ] && continue
    echo "orphan $q"
  done
fi
"#
    )
}

/// The script that removes the queue `$1`: only a 16-hex name directly under a `land-queue`, holding no ticket in any area and no lander lock.
pub const REMOVE_QUEUE: &str = r#"set -u
q=$1
name=${q##*/}
parent=${q%/*}
[ "${parent##*/}" = land-queue ] || { echo "not a queue under a land-queue: $q" >&2; exit 1; }
[ "${#name}" = 16 ] || { echo "not a queue key: $name" >&2; exit 1; }
case $name in *[!0-9a-f]*) echo "not a queue key: $name" >&2; exit 1 ;; esac
[ -d "$q" ] && [ ! -L "$q" ] || { echo "not a queue directory: $q" >&2; exit 1; }
[ -f "$q/lander.lock" ] && { echo "a lander serves $q" >&2; exit 1; }
find "$q" -type f -name '*.json' -print -quit | grep -q . && { echo "$q holds a ticket" >&2; exit 1; }
rm -rf -- "$q"
echo "removed $q"
"#;

pub fn parse_probe(text: &str) -> Result<Probe, String> {
    let mut probe = Probe::default();
    let mut disk = None;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["disk", used, available] => {
                let (used, available): (f64, f64) = (used.parse().map_err(|_| format!("disk line {line:?}"))?, available.parse().map_err(|_| format!("disk line {line:?}"))?);
                disk = Some((available * 100.0 / (used + available).max(1.0), available / (1u64 << 20) as f64));
            }
            ["slot", slot, held, pgid, state @ ("locked" | "free")] => probe.slots.push(SlotHold { slot: (*slot).to_owned(), held: held.parse().map_err(|_| format!("slot line {line:?}"))?, pgid: pgid.parse().ok().filter(|pgid| *pgid > 1), locked: *state == "locked" }),
            ["prunable", count, repo] => probe.prunable.push((PathBuf::from(repo), count.parse().map_err(|_| format!("prunable line {line:?}"))?)),
            ["orphan", queue] => probe.orphaned.push(PathBuf::from(queue)),
            _ => {}
        }
    }
    let (free_percent, free_gib) = disk.ok_or_else(|| "the probe printed no disk line".to_owned())?;
    probe.free_percent = free_percent;
    probe.free_gib = free_gib;
    Ok(probe)
}

