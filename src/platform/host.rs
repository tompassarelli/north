pub(crate) fn name() -> String {
    std::env::var("RESIDENT_CAP_HOST").unwrap_or_else(|_| std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().into())
}
