//! The two signals that say a host cannot take a new session, read from the host itself: its memory and its root
//! disk. A session is admitted on memory and slot room only, never on the build queue or load, because the queue
//! and the load average count jobs and threads that are mostly idle CLIs; builds wait for their own pressure target
//! (`pressure_target`), not the launch of the session that runs them. Placement skips such a host at launch, and
//! alerts on a stalled fleet read the same signals through `Saturation::refusal`.
use super::admission::{self, Class, default_mib as default};

const DISK_FLOOR: &str = "build_disk_free_gib";
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const KIB_PER_MIB: f64 = 1024.0;

/// Prints, one per line: `MemTotal` and `MemAvailable` in kB, then `df -Pk /`.
pub(crate) const PROBE: &str = "awk '/^MemTotal:|^MemAvailable:/ {print $2}' /proc/meminfo; df -Pk / | tail -1";

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct Saturation {
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    pub disk_free_gib: f64,
}

pub(crate) fn parse(text: &str) -> Option<Saturation> {
    let mut lines = text.lines();
    let total_kib: f64 = lines.next()?.trim().parse().ok()?;
    let available_kib: f64 = lines.next()?.trim().parse().ok()?;
    let kib: f64 = lines.next()?.split_whitespace().nth(3)?.parse().ok()?;
    Some(Saturation { memory_total_mib: total_kib / KIB_PER_MIB, memory_available_mib: available_kib / KIB_PER_MIB, disk_free_gib: kib / KIB_PER_GIB })
}

impl Saturation {
    /// Why `host` takes no new session, the first of: too little available memory beyond a session's own request, or a root disk under the build floor.
    pub(crate) fn refusal(&self, host: &str) -> Option<String> {
        let request = admission::dispatch_request_mib(Class::Agent);
        if admission::memory_short(self.memory_total_mib, self.memory_available_mib, request) {
            return Some(format!("{host} skipped: {:.1} GiB of memory available, too little beside a {:.1} GiB session", self.memory_available_mib / 1024.0, request / 1024.0));
        }
        let floor = default(DISK_FLOOR);
        (self.disk_free_gib < floor).then(|| format!("{host} skipped: {:.1} GiB free on the root disk, under {floor}", self.disk_free_gib))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const READING: &str = "98643000\n40960000\n/dev/md127 489970656 443351304 21656744 96% /\n";

    fn calm() -> Saturation {
        Saturation { memory_total_mib: 96000.0, memory_available_mib: 55000.0, disk_free_gib: 40.0 }
    }

    #[test]
    fn a_session_is_refused_a_host_for_memory_or_disk_never_for_its_queue_or_load() {
        let reading = parse(READING).unwrap();
        assert!((reading.memory_total_mib - 96331.05).abs() < 0.01);
        assert!((reading.memory_available_mib - 40000.0).abs() < 0.01);
        assert!((reading.disk_free_gib - 20.65).abs() < 0.01);
        assert_eq!(parse("not a reading"), None);
        assert_eq!(calm().refusal("gex45"), None);
        let short = Saturation { memory_available_mib: 8900.0, ..calm() };
        assert!(short.refusal("gex45").unwrap().starts_with("gex45 skipped: 8.7 GiB of memory available"));
        assert_eq!(Saturation { disk_free_gib: 15.0, ..calm() }.refusal("gex45"), None);
        assert_eq!(Saturation { disk_free_gib: 14.2, ..calm() }.refusal("gex45").unwrap(), "gex45 skipped: 14.2 GiB free on the root disk, under 15");
        let worst = Saturation { memory_available_mib: 1000.0, disk_free_gib: 1.0, ..calm() };
        assert!(worst.refusal("h").unwrap().contains("memory"));
    }
}
