pub fn name() -> String {
    std::env::var("RESIDENT_CAP_HOST").ok().filter(|name| !name.is_empty()).unwrap_or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().into())
}

/// When this machine booted, in unix milliseconds, from the `btime` line of `/proc/stat`.
pub fn booted_ms() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let seconds: u64 = stat.lines().find_map(|line| line.strip_prefix("btime "))?.trim().parse().ok()?;
    Some(seconds * 1000)
}

/// Run-queue load per hardware thread right now: the larger of the 1- and 5-minute load averages over the thread count.
pub fn contention() -> Option<f64> {
    let threads = std::thread::available_parallelism().ok()?.get();
    contention_of(&std::fs::read_to_string("/proc/loadavg").ok()?, threads)
}

pub fn contention_of(loadavg: &str, threads: usize) -> Option<f64> {
    let mut averages = loadavg.split_whitespace().map(|field| field.parse::<f64>().ok());
    let one = averages.next()??;
    let five = averages.next()??;
    (threads > 0).then(|| one.max(five) / threads as f64)
}
