use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use north_landing::land::after::{follow, send};
use north_landing::land::watchdog::{self, Hands as _};
use north_landing::land::{promote, queue};

use crate::bounds::Remedy;
use crate::facts::shell;

pub const PROXY: &str = "proxy";
pub const NOTICED_BY: &str = "lander-doctor";

/// The doctor's effects on the hosts: applying a remedy, recording an incident row and paging the proxy.
pub trait Hands {
    fn apply(&self, remedy: &Remedy) -> Result<String, String>;
    fn incident(&self, class: &str, key: &str, subject: &str, detail: &str) -> bool;
    fn page(&self, text: &str) -> bool;
}

pub struct Host;

const PRUNE: &str = r#"git -C "$1" worktree prune --verbose 2>&1 | grep -c . || true"#;

pub const RELEASE: &str = r#"root=${3:-${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}}
base=$root/$1 pgid=$2
case $pgid in "" | *[!0-9]*) echo "process group $pgid is not a number" >&2; exit 1 ;; esac
[ "$(cat "$base.pgid" 2>/dev/null)" = "$pgid" ] || { echo "slot $1 no longer names process group $pgid" >&2; exit 1; }
if (exec 7>>"$base.lock"; flock -n 7) 2>/dev/null; then echo "slot $1 is no longer held" >&2; exit 1; fi
kill -TERM "-$pgid" 2>/dev/null || true
i=0
while [ "$i" -lt 20 ] && kill -0 "-$pgid" 2>/dev/null; do sleep 0.1; i=$((i + 1)); done
kill -KILL "-$pgid" 2>/dev/null || true
rm -f "$base.owner" "$base.pgid" "$base.holder"
echo "ended process group $pgid"
"#;

const EVICT: &str = r#"PATH=$HOME/.local/bin:$PATH
north capacity reap-orphans >/dev/null 2>&1 || true
df -Pk "$HOME" | awk 'NR == 2 { printf "%.0f%% free after eviction\n", $4 * 100 / ($3 + $4) }'
"#;

impl Hands for Host {
    fn apply(&self, remedy: &Remedy) -> Result<String, String> {
        match remedy {
            Remedy::Prune { place, repo } => shell(place, PRUNE, &[&repo.to_string_lossy()]).map(|pruned| format!("pruned {} registration(s)", pruned.trim())),
            Remedy::Release { place, slot, pgid } => shell(place, RELEASE, &[slot, &pgid.to_string()]).map(|said| said.trim().to_owned()),
            Remedy::Rerun { queue, ticket } => rerun(queue, ticket),
            Remedy::Evict { place } => shell(place, EVICT, &[]).map(|said| said.trim().to_owned()),
            Remedy::Restart { unit } => watchdog::Host.restart(unit, "the lander doctor read it wedged").map(|()| format!("restarted {unit}")),
            Remedy::Rollback { home, from, to, units } => roll_back(home, from, to, units),
            Remedy::Wait(reason) => Ok(reason.clone()),
        }
    }

    fn incident(&self, class: &str, key: &str, subject: &str, detail: &str) -> bool {
        Command::new(follow::program())
            .args(["incident", "record", "--key", key, "--class", class, "--noticed-by", NOTICED_BY, "--subject", subject, "--detail", detail])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn page(&self, text: &str) -> bool {
        send(PROXY, text)
    }
}

/// Moves a refused ticket back into its queue once, leaving a marker so its next refusal is final.
pub fn rerun(queue_dir: &Path, ticket: &Path) -> Result<String, String> {
    let name = ticket.file_name().ok_or_else(|| format!("{} names no file", ticket.display()))?;
    let mut decided = queue::read(ticket).map_err(|error| format!("{}: {error}", ticket.display()))?;
    decided.verdict = None;
    decided.decided_on = None;
    let marker = queue_dir.join(crate::RERAN).join(name);
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&marker, format!("{}\n", ticket.display())).map_err(|error| format!("{}: {error}", marker.display()))?;
    queue::write(&queue_dir.join(name), &decided).map_err(|error| format!("{}: {error}", queue_dir.display()))?;
    fs::remove_file(ticket).map_err(|error| format!("{}: {error}", ticket.display()))?;
    Ok(format!("requeued {} ({})", decided.branch, name.to_string_lossy()))
}

/// Points the lander link at `to`, names `from` refused so north land promote leaves it off the lander, and restarts each lander unit.
pub fn roll_back(home: &Path, from: &str, to: &str, units: &[String]) -> Result<String, String> {
    let refused = home.join(promote::REFUSED);
    fs::write(&refused, format!("{from}\n")).map_err(|error| format!("{}: {error}", refused.display()))?;
    promote::swap(&home.join("lander"), Path::new(to)).map_err(|error| format!("{}: {error}", home.join("lander").display()))?;
    let restarted: Vec<String> = units.iter().filter(|unit| watchdog::Host.unit_exists(unit)).filter_map(|unit| watchdog::Host.restart(unit, "the lander doctor rolled the lander back").ok().map(|()| unit.clone())).collect();
    Ok(format!("the lander link names {to}; restarted {}", if restarted.is_empty() { "no unit".to_owned() } else { restarted.join(", ") }))
}
