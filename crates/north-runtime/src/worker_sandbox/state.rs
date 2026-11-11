use std::io::Read;
use std::path::{Path, PathBuf};

/// The agent state under `~/.local/state` a worker command writes, and nothing beside it: the hooks' records under `agents`, the conversation index `north convo search` keeps, the threads ledger, which a worker on the ledger host writes a claim and a box check into, the threads spool and its records, which a worker elsewhere queues them in, the fleet spool a report, a gate and a spend request go through, the guard's own logs and stamps, the build shims' metadata and timings and the dev shells' roots, and `north/land`, where a land check takes its locks. The runtimes unconfined sessions execute, the fleet's session records, the lifecycle ledger, the client profiles and every other store stay unwritable.
const WRITTEN: [&str; 16] = [
    "agents",
    "convo",
    "threads",
    "north/fleet/spool",
    "north/land",
    "north/file-skills",
    "north/recommend-acts",
    "north/gates",
    "north/decisions",
    "north/reader-stamps",
    "north/nix-cards",
    "north/cargo-metadata",
    "north/dev-shells",
    "north/tripwire.log",
    "north/build-times.tsv",
    "north/break-glass.jsonl",
];

/// The stores a worker command may write but never remove from, since the history the fleet screen shows and every later decision reads lives in them.
const KEPT: [&str; 3] = ["convo", "threads", "north/fleet"];

/// Every path under `state` a worker command may open for writing or create beneath: the stores [`WRITTEN`] names and the shared-memory index of each WAL database directly in `north` (the lifecycle ledger and the graph), which it reads but never writes, and which SQLite writes even to read.
pub fn writable(state: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = WRITTEN.iter().map(|store| state.join(store)).collect();
    paths.extend(write_ahead_logs(&state.join("north")));
    paths
}

/// The directories under `state` a worker command may remove or rename files out of: those it writes, apart from the stores [`KEPT`].
pub fn removable(state: &Path) -> Vec<PathBuf> {
    let kept: Vec<PathBuf> = KEPT.iter().map(|store| state.join(store)).collect();
    WRITTEN.iter().map(|store| state.join(store)).filter(|store| store.is_dir() && !kept.iter().any(|kept| store.starts_with(kept))).collect()
}

/// Makes the write-ahead log and shared-memory files of each WAL database directly in `directory`, empty as SQLite leaves them, where they are missing, and returns the shared-memory ones: a reader cannot create them inside the domain, and with the database and its log unwritable it opens read-only and needs only the shared-memory index.
fn write_ahead_logs(directory: &Path) -> Vec<PathBuf> {
    let databases = std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "db") && in_wal_mode(path));
    let mut shared = Vec::new();
    for database in databases {
        for suffix in ["-wal", "-shm"] {
            let mut name = database.clone().into_os_string();
            name.push(suffix);
            let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(PathBuf::from(&name));
            if suffix == "-shm" {
                shared.push(PathBuf::from(name));
            }
        }
    }
    shared
}

/// SQLite's file format records WAL mode as 2 in the read and write version bytes at offsets 18 and 19 of the header.
fn in_wal_mode(database: &Path) -> bool {
    let mut header = [0u8; 20];
    std::fs::File::open(database).and_then(|mut file| file.read_exact(&mut header)).is_ok() && header[18..20] == [2, 2]
}
