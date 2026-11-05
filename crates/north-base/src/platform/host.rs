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

#[cfg(test)]
mod tests {
    use super::contention_of;

    #[test]
    fn contention_is_the_larger_load_average_per_thread() {
        assert_eq!(contention_of("25.00 10.00 5.00 3/900 123\n", 20), Some(1.25));
        assert_eq!(contention_of("2.00 10.00 5.00 3/900 123\n", 20), Some(0.5));
        assert_eq!(contention_of("garbage", 20), None);
        assert_eq!(contention_of("1.00 1.00 1.00 1/1 1", 0), None);
    }
}
