pub(crate) fn name() -> String {
    std::env::var("RESIDENT_CAP_HOST").ok().filter(|name| !name.is_empty()).unwrap_or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().into())
}
