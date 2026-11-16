//! The read of what fills a host's disk: `du` one level deep over the fixed roots under the operator's home, each under a time limit, so a caller's only word is the host.

pub const ROOTS: [&str; 5] = [".local/state", ".local/state/north", ".local/share", ".cache", "code"];
pub const SECONDS: &str = "300";
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;

/// One `du -k` row as `SIZE GiB  PATH`.
pub fn row(kib: u64, path: &str) -> String {
    format!("{:>9.2} GiB  {path}", kib as f64 / KIB_PER_GIB)
}

/// The rows of `du -k` output as the size in KiB and the path, in the order told.
pub fn parse(du: &str) -> Vec<(u64, String)> {
    du.lines().filter_map(|line| line.split_once('\t')).filter_map(|(kib, path)| Some((kib.trim().parse().ok()?, path.to_owned()))).collect()
}

/// The `du -k` rows in `du`, largest first, each as `SIZE GiB  PATH`.
pub fn largest_first(du: &str) -> String {
    let mut rows: Vec<(u64, &str)> = du.lines().filter_map(|line| line.split_once('\t')).filter_map(|(kib, path)| Some((kib.trim().parse().ok()?, path))).collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    rows.iter().map(|(kib, path)| row(*kib, path)).collect::<Vec<_>>().join("\n")
}

/// The lines of a disk read at or over 1 GiB, in the order told.
pub fn over_gib(told: &str) -> Vec<String> {
    told.lines().filter(|line| line.split_once(" GiB  ").and_then(|(size, _)| size.trim().parse::<f64>().ok()).is_some_and(|gib| gib >= 1.0)).map(str::to_owned).collect()
}
