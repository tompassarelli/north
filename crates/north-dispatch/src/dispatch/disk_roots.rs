//! What fills this dispatch host's disk, read here with no ssh: each directory one level under the fixed roots below the operator's home, and the package store beside them, each read under a time limit of its own.
use north_base::platform::disk_read::{self, ROOTS, SECONDS, over_gib, row};
use std::path::Path;
use std::process::Command;

const PACKAGE_STORE: &str = "/nix/store";

type Sizes = (Vec<(u64, String)>, Vec<String>);

/// Each directory one level under the fixed roots below `home` holding at least 1 GiB, largest first, and the package store's size beside them; a read that does not finish within `SECONDS` or fails reads as one line saying so.
pub(super) fn local(home: &Path) -> Vec<String> {
    let home = home.to_string_lossy().trim_end_matches('/').to_owned();
    let (mut rows, lines): Sizes = std::thread::scope(|scope| {
        let roots: Vec<_> = ROOTS
            .iter()
            .map(|root| {
                let path = format!("{home}/{root}");
                scope.spawn(move || sized_root(&path))
            })
            .collect();
        let store = scope.spawn(|| sized_dir(PACKAGE_STORE));
        roots.into_iter().chain(std::iter::once(store)).filter_map(|handle| handle.join().ok()).fold((Vec::new(), Vec::new()), gather)
    });
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    rows.dedup_by(|a, b| a.1 == b.1);
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    over_gib(&rows.iter().map(|(kib, path)| row(*kib, path)).collect::<Vec<_>>().join("\n")).into_iter().chain(lines).collect()
}

fn gather((mut rows, mut lines): Sizes, (more, notes): Sizes) -> Sizes {
    rows.extend(more);
    lines.extend(notes);
    (rows, lines)
}

fn du(path: &str, depth: &str) -> Option<Vec<(u64, String)>> {
    let output = Command::new("timeout").args([SECONDS, "du", "-k", "-x", "-d", depth, path]).output().ok().filter(|output| output.status.success())?;
    Some(disk_read::parse(&String::from_utf8_lossy(&output.stdout)))
}

fn failed(path: &str) -> String {
    format!("du of {path} did not finish within {SECONDS} s or failed")
}

fn sized_dir(path: &str) -> Sizes {
    match du(path, "0") {
        Some(rows) => (rows, Vec::new()),
        None => (Vec::new(), vec![failed(path)]),
    }
}

/// A root's directories one level down; a root whose `du` does not finish is sized child by child, each child within `SECONDS` of its own.
fn sized_root(path: &str) -> Sizes {
    if let Some(rows) = du(path, "1") {
        return (rows, Vec::new());
    }
    let children: Vec<String> = std::fs::read_dir(path).into_iter().flatten().flatten().map(|entry| entry.path().to_string_lossy().into_owned()).collect();
    if children.is_empty() {
        return (Vec::new(), vec![failed(path)]);
    }
    let (rows, mut lines) = std::thread::scope(|scope| {
        let handles: Vec<_> = children.iter().map(|child| scope.spawn(move || sized_dir(child))).collect();
        handles.into_iter().filter_map(|handle| handle.join().ok()).fold((Vec::new(), Vec::new()), gather)
    });
    lines.insert(0, format!("du of {path} did not finish within {SECONDS} s; sized child by child"));
    (rows, lines)
}
