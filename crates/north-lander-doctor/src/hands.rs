use std::fs;
use std::path::Path;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use north_lander::land::after::{follow, send};
use north_base::platform::land_notice::OWNER;
use north_lander::land::watchdog::{self, Hands as _};
use north_lander::land::{promote, queue};

use crate::bounds::Remedy;
use north_check_environment::Environment;
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

const RELEASE: &str = r#"@SLOT_HELD@root=${3:-${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}}
base=$root/$1 pgid=$2
case $pgid in "" | *[!0-9]*) echo "process group $pgid is not a number" >&2; exit 1 ;; esac
[ "$(cat "$base.pgid" 2>/dev/null)" = "$pgid" ] || { echo "slot $1 no longer names process group $pgid" >&2; exit 1; }
if ! slot_held "$base.lock"; then echo "slot $1 is no longer held" >&2; exit 1; fi
kill -TERM "-$pgid" 2>/dev/null || true
i=0
while [ "$i" -lt 20 ] && kill -0 "-$pgid" 2>/dev/null; do sleep 0.1; i=$((i + 1)); done
kill -KILL "-$pgid" 2>/dev/null || true
rm -f "$base.owner" "$base.pgid" "$base.holder"
echo "ended process group $pgid"
"#;

pub const CLEAR: &str = r#"root=${2:-${NORTH_OFFLOAD_ROOT:-$HOME/.cache/land-offload}}
base=$root/$1
exec 7>>"$base.lock"
flock -n 7 || { echo "slot $1 is held" >&2; exit 1; }
rm -f "$base.owner" "$base.pgid" "$base.holder"
echo "removed the holder files of slot $1, whose lock is free"
"#;

/// The script that ends the process group a held slot names: `SLOT PGID [ROOT]`.
pub fn release() -> String {
    RELEASE.replace("@SLOT_HELD@", north_base::foundation::offload::SLOT_HELD_SH)
}

const EVICT: &str = r#"PATH=$HOME/.local/bin:$PATH
north capacity disk keep >/dev/null 2>&1 || true
df -Pk "$HOME" | awk 'NR == 2 { printf "%.0f%% free after eviction\n", $4 * 100 / ($3 + $4) }'
"#;

impl Hands for Host {
    fn apply(&self, remedy: &Remedy) -> Result<String, String> {
        match remedy {
            Remedy::Prune { place, repo } => shell(place, PRUNE, &[&repo.to_string_lossy()]).map(|pruned| format!("pruned {} registration(s)", pruned.trim())),
            Remedy::Release { place, slot, pgid } => shell(place, &release(), &[slot, &pgid.to_string()]).map(|said| said.trim().to_owned()),
            Remedy::Clear { place, slot } => shell(place, CLEAR, &[slot]).map(|said| said.trim().to_owned()),
            Remedy::Rerun { queue, ticket } => rerun(queue, ticket),
            Remedy::RemoveQueue { queue } => remove_queue(queue),
            Remedy::Evict { place } => shell(place, EVICT, &[]).map(|said| said.trim().to_owned()),
            Remedy::Restart { unit } => watchdog::Host.restart(unit, "the lander doctor read it wedged").map(|()| format!("restarted {unit}")),
            Remedy::Rollback { home, from, to, units } => roll_back(home, from, to, units),
            Remedy::Alarm(text) | Remedy::Page(text) => if self.page(text) { Ok("the proxy heard of it".to_owned()) } else { Err("paging the proxy failed".to_owned()) },
            Remedy::Promote => promote(),
            Remedy::CheckHere { queue } => {
                north_check_environment::settle(queue, Environment::Local, "the lander doctor read main's check environment broken past its bound");
                Ok("new candidates are judged on this host until the placement is probed again".to_owned())
            }
            Remedy::Wait(reason) => Ok(reason.clone()),
        }
    }

    fn incident(&self, class: &str, key: &str, subject: &str, detail: &str) -> bool {
        Command::new(follow::program())
            .args(["ledger", "incident", "record", "--key", key, "--class", class, "--noticed-by", NOTICED_BY, "--subject", subject, "--detail", detail])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn page(&self, text: &str) -> bool {
        let told = [PROXY, OWNER].map(|session| send(session, text));
        told.contains(&true)
    }
}

/// Starts `north runtime promote` detached, logging where a landing's promotion logs, so a retry while another promotion runs leaves the newest runtime to that one.
pub fn promote() -> Result<String, String> {
    let log = north_lander::land::lander::state_home().join("north/runtime-follow.log");
    let file = fs::OpenOptions::new().create(true).append(true).open(&log).map_err(|error| format!("{}: {error}", log.display()))?;
    let child = Command::new(follow::program()).args(["runtime", "promote"]).stdin(Stdio::null()).stdout(file.try_clone().map_err(|error| error.to_string())?).stderr(file).process_group(0).spawn().map_err(|error| format!("north runtime promote: {error}"))?;
    Ok(format!("started north runtime promote (pid {}), logging to {}", child.id(), log.display()))
}

/// Moves a refused ticket back into its queue once, so its next refusal is final: its record keeps the rerun.
pub fn rerun(queue_dir: &Path, ticket: &Path) -> Result<String, String> {
    let name = ticket.file_name().ok_or_else(|| format!("{} names no file", ticket.display()))?;
    let requeued = queue::requeue(queue_dir, ticket, "the lander doctor reran an infrastructure refusal").map_err(|error| format!("{}: {error}", ticket.display()))?;
    let branch = queue::read(&requeued).map(|decided| decided.branch).unwrap_or_default();
    Ok(format!("requeued {branch} ({})", name.to_string_lossy()))
}

/// Removes a queue under this host's land-queue directory that holds no ticket and that no checkout serves.
fn remove_queue(queue: &Path) -> Result<String, String> {
    let root = north_lander::land::lander::state_home().join("north/land-queue");
    remove_queue_in(&root, queue)
}

pub(crate) fn remove_queue_in(root: &Path, queue: &Path) -> Result<String, String> {
    if queue.parent() != Some(root) {
        return Err(format!("{} is not a queue directly under {}", queue.display(), root.display()));
    }
    if !north_lander::land::repositories::orphan(queue) {
        return Err(format!("{} holds a ticket, so its history stays", queue.display()));
    }
    fs::remove_dir_all(queue).map_err(|error| format!("{}: {error}", queue.display()))?;
    Ok(format!("removed the land queue {} no checkout serves and no ticket waits in", queue.display()))
}

/// Points the lander link at `to`, names `from` refused so north land promote leaves it off the lander, and restarts each lander unit.
pub fn roll_back(home: &Path, from: &str, to: &str, units: &[String]) -> Result<String, String> {
    let refused = home.join(promote::REFUSED);
    fs::write(&refused, format!("{from}\n")).map_err(|error| format!("{}: {error}", refused.display()))?;
    promote::swap(&home.join("lander"), Path::new(to)).map_err(|error| format!("{}: {error}", home.join("lander").display()))?;
    let restarted: Vec<String> = units.iter().filter(|unit| watchdog::Host.unit_exists(unit)).filter_map(|unit| watchdog::Host.restart(unit, "the lander doctor rolled the lander back").ok().map(|()| unit.clone())).collect();
    Ok(format!("the lander link names {to}; restarted {}", if restarted.is_empty() { "no unit".to_owned() } else { restarted.join(", ") }))
}
