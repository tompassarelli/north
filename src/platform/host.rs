pub(crate) fn name() -> String {
    std::env::var("RESIDENT_CAP_HOST").ok().filter(|name| !name.is_empty()).unwrap_or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().into())
}

/// When this machine booted, in unix milliseconds, from the `btime` line of `/proc/stat`.
pub(crate) fn booted_ms() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let seconds: u64 = stat.lines().find_map(|line| line.strip_prefix("btime "))?.trim().parse().ok()?;
    Some(seconds * 1000)
}
