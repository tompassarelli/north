//! The read of what fills a host's disk: `du` one level deep over the fixed roots under the operator's home, each under a time limit, so a caller's only word is the host.
use std::path::Path;
use std::process::Command;

pub const ROOTS: [&str; 5] = [".local/state", ".local/state/north", ".local/share", ".cache", "code"];
pub const SECONDS: &str = "300";
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;

fn row(kib: u64, path: &str) -> String {
    format!("{:>9.2} GiB  {path}", kib as f64 / KIB_PER_GIB)
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

/// Each directory one level under the fixed roots below `home` holding at least 1 GiB, largest first, read on this host with no ssh; a root that does not finish within `SECONDS` or fails reads as one line saying so.
pub fn local(home: &Path) -> Vec<String> {
    let reads: Vec<(String, Option<String>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = ROOTS
            .iter()
            .map(|root| {
                let path = format!("{}/{root}", home.to_string_lossy().trim_end_matches('/'));
                scope.spawn(move || {
                    let output = Command::new("timeout").args([SECONDS, "du", "-k", "-x", "-d", "1", &path]).output().ok().filter(|output| output.status.success());
                    (path, output.map(|output| String::from_utf8_lossy(&output.stdout).into_owned()))
                })
            })
            .collect();
        handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
    });
    let mut rows: Vec<(u64, String)> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for (path, du) in &reads {
        match du {
            Some(du) => rows.extend(du.lines().filter_map(|line| line.split_once('\t')).filter_map(|(kib, path)| Some((kib.trim().parse().ok()?, path.to_owned())))),
            None => lines.push(format!("du of {path} did not finish within {SECONDS} s or failed")),
        }
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    rows.dedup_by(|a, b| a.1 == b.1);
    over_gib(&rows.iter().map(|(kib, path)| row(*kib, path)).collect::<Vec<_>>().join("\n")).into_iter().chain(lines).collect()
}
