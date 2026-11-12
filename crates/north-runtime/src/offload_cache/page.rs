//! The one page a host-disk episode sends the proxy and records as an incident: what the pass took and, when the host stays closed, what holds the space.
use std::process::{Command, Stdio};

use super::{ALARM_STORES, Disk, Evicted, GIB, INCIDENT_CLASS, Line};
use crate::platform::disk_ledger;

/// The ledger's `ALARM_STORES` largest stores as `NAME (KIND, N GiB, trim: HOW)`, then the bytes no store explains.
pub(super) fn largest(account: &disk_ledger::Account) -> String {
    if account.reflinked {
        return format!("{:.0} GiB used on a filesystem that shares extents, so its stores are not summed", account.used as f64 / GIB);
    }
    let stores: Vec<String> = account.top(ALARM_STORES).iter().map(|row| format!("{} ({}, {:.0} GiB, trim: {})", row.entry.name, row.entry.kind.word(), row.bytes as f64 / GIB, row.entry.trim)).collect();
    format!("{}; {:.0} GiB unaccounted", stores.join(", "), account.unaccounted() as f64 / GIB)
}

/// The one page of a pass that `before` started under the held line: what it took, and when `after` still reads under `closed`, that the
/// host stays closed to dispatch with nothing more the rules may reclaim and which stores hold the space.
pub fn alarm(host: &str, before: Disk, after: Disk, (closed, held): (Line, Line), evicted: &[Evicted], largest: &str) -> String {
    let freed = evicted.iter().map(|gone| gone.bytes).sum::<u64>() as f64 / GIB;
    let size_gib = after.size_bytes as f64 / GIB;
    let target = Line::low_watermark().floor_gib(size_gib);
    let bound = closed.floor_gib(size_gib);
    let crossed = if before.past(closed) { format!("the {bound:.0} GiB free it is closed to dispatch under") } else { format!("the {:.0} GiB free its reaper holds it over", held.floor_gib(size_gib)) };
    let short = if after.past(closed) {
        format!(", so it stays closed to dispatch under its {bound:.0} GiB disk bound with nothing more its rules may reclaim; what holds the space")
    } else if after.free_gib < target {
        ", short of it with nothing regenerable left, so the rest sits in the stores below; largest stores".to_owned()
    } else {
        "; largest stores".to_owned()
    };
    format!(
        "{host} root disk passed {crossed} at {:.0}% ({:.0} GiB free); one cleanup pass took {} entries ({freed:.0} GiB) toward the low watermark of {target:.0} GiB free and left {:.0} GiB free{short}: {largest}",
        before.used_percent,
        before.free_gib,
        evicted.len(),
        after.free_gib
    )
}

pub(super) fn page(detail: &str) -> bool {
    let north = north_base::platform::north_binary::north_binary();
    let sent = Command::new(north).args(["fleet", "send", "--from", "service:north-lease-reaper", "proxy", detail]).stdout(Stdio::null()).status();
    sent.is_ok_and(|status| status.success())
}

pub(super) fn record(key: &str, host: &str, detail: &str) -> bool {
    let north = north_base::platform::north_binary::north_binary();
    let recorded = Command::new(north)
        .args(["ledger", "incident", "record", "--key", key, "--class", INCIDENT_CLASS, "--noticed-by", "lease-reaper", "--subject", host, "--detail", detail])
        .stdout(Stdio::null())
        .status();
    recorded.is_ok_and(|status| status.success())
}
