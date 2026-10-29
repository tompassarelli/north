//! The three signals that say a host is saturated for new work, read from the host itself: the
//! machine-capacity admission queue, its root disk and its five-minute load. Placement skips such a
//! host at launch, and alerts on a stalled fleet read the same signals through `Saturation::refusal`.
use super::admission::default_mib as default;

pub(crate) const QUEUE_LIMIT: &str = "launch_queue_limit";
const DISK_FLOOR: &str = "build_disk_free_gib";
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;

/// Prints, one per line: the admission queue's tickets, `nproc`, `/proc/loadavg`, then `df -Pk /`.
pub(crate) const PROBE: &str = "d=\"${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/agent-capacity-v1/queue\"; find \"$d\" -maxdepth 1 -type f ! -name \"*.tmp\" 2>/dev/null | wc -l; nproc; cat /proc/loadavg; df -Pk / | tail -1";

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct Saturation {
    pub queued: u32,
    pub cores: f64,
    pub load5: f64,
    pub disk_free_gib: f64,
}

pub(crate) fn parse(text: &str) -> Option<Saturation> {
    let mut lines = text.lines();
    let queued = lines.next()?.trim().parse().ok()?;
    let cores = lines.next()?.trim().parse().ok()?;
    let load5 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let kib: f64 = lines.next()?.split_whitespace().nth(3)?.parse().ok()?;
    Some(Saturation { queued, cores, load5, disk_free_gib: kib / KIB_PER_GIB })
}

impl Saturation {
    /// Why `host` takes no new work, the first of: a full admission queue, a root disk under the
    /// build floor, a five-minute load over its cores.
    pub(crate) fn refusal(&self, host: &str) -> Option<String> {
        let (limit, floor) = (default(QUEUE_LIMIT), default(DISK_FLOOR));
        if f64::from(self.queued) >= limit {
            return Some(format!("{host} skipped: {} queued for admission", self.queued));
        }
        if self.disk_free_gib < floor {
            return Some(format!("{host} skipped: {:.1} GiB free on the root disk, under {floor}", self.disk_free_gib));
        }
        (self.load5 > self.cores).then(|| format!("{host} skipped: 5-minute load {:.1} over {} cores", self.load5, self.cores))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const READING: &str = "9\n20\n17.86 28.95 34.41 21/3765 1874169\n/dev/md127 489970656 443351304 21656744 96% /\n";

    fn calm() -> Saturation {
        Saturation { queued: 0, cores: 20.0, load5: 3.0, disk_free_gib: 40.0 }
    }

    #[test]
    fn a_probe_reading_gives_queue_cores_five_minute_load_and_root_disk() {
        let reading = parse(READING).unwrap();
        assert_eq!((reading.queued, reading.cores, reading.load5), (9, 20.0, 28.95));
        assert!((reading.disk_free_gib - 20.65).abs() < 0.01);
        assert_eq!(parse("not a reading"), None);
    }

    #[test]
    fn a_queue_of_four_or_more_names_the_admission_queue() {
        assert_eq!(Saturation { queued: 3, ..calm() }.refusal("gex45"), None);
        assert_eq!(Saturation { queued: 9, ..calm() }.refusal("gex45").unwrap(), "gex45 skipped: 9 queued for admission");
        assert!(Saturation { queued: 4, ..calm() }.refusal("gex45").is_some());
    }

    #[test]
    fn a_root_disk_under_the_build_floor_names_the_disk() {
        assert_eq!(Saturation { disk_free_gib: 15.0, ..calm() }.refusal("gex45"), None);
        assert_eq!(Saturation { disk_free_gib: 14.2, ..calm() }.refusal("gex45").unwrap(), "gex45 skipped: 14.2 GiB free on the root disk, under 15");
    }

    #[test]
    fn a_five_minute_load_over_the_cores_names_the_load() {
        assert_eq!(Saturation { load5: 20.0, ..calm() }.refusal("gex45"), None);
        assert_eq!(Saturation { load5: 29.5, ..calm() }.refusal("gex45").unwrap(), "gex45 skipped: 5-minute load 29.5 over 20 cores");
    }

    #[test]
    fn the_queue_is_named_before_the_disk_and_the_disk_before_the_load() {
        let worst = Saturation { queued: 9, disk_free_gib: 1.0, load5: 99.0, ..calm() };
        assert!(worst.refusal("h").unwrap().contains("queued"));
        assert!(Saturation { queued: 0, ..worst }.refusal("h").unwrap().contains("GiB free"));
    }
}
