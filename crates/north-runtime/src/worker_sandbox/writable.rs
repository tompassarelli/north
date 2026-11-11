use std::io::Read;
use std::path::{Path, PathBuf};

use super::Context;
use super::except::{Entries, beneath_except};
use super::removable::projects;

/// The stores under `~/.local/state` no worker command may write: the client profiles, whose settings, hooks, skills and memory every session on an account loads, the unconfined ones included, and the setup tokens.
const CLIENT_STATE: [&str; 2] = ["north/profiles", "north/claude-tokens"];

/// Every path a worker command may open for writing or create a file, directory, link or node beneath: what it may remove (its lane, its git state, scratch, caches and build output), the device files a shell writes to, the agent state apart from the client profiles and setup tokens (the fleet and threads spools, the conversation index, the ledgers), and the worktrees directory of each project it works in, where `north lane new` and `git worktree add` create a lane. The operator's dotfiles and configuration, the local bin directories, the client profiles, every main checkout's files and every other project stay unwritable.
pub fn writable(context: &Context, removable: &[PathBuf]) -> Vec<PathBuf> {
    let state = context.home.join(".local/state");
    let client: Vec<PathBuf> = CLIENT_STATE.iter().map(|store| state.join(store)).collect();
    write_ahead_logs(&state.join("north"));
    let mut paths: Vec<PathBuf> = removable.to_vec();
    paths.push(PathBuf::from("/dev"));
    paths.extend(beneath_except(&state, &client, Entries::All));
    paths.extend(projects(context).into_iter().map(|project| project.join("worktrees")));
    paths
}

/// Makes the write-ahead log and shared-memory files of each WAL database directly in `directory`, empty as SQLite leaves them, where they are missing: a Landlock rule that let a worker create them there would let it create files beneath the client profiles in the same directory too.
fn write_ahead_logs(directory: &Path) {
    for database in std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "db") && in_wal_mode(path)) {
        for suffix in ["-wal", "-shm"] {
            let mut name = database.clone().into_os_string();
            name.push(suffix);
            let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(PathBuf::from(name));
        }
    }
}

/// SQLite's file format records WAL mode as 2 in the read and write version bytes at offsets 18 and 19 of the header.
fn in_wal_mode(database: &Path) -> bool {
    let mut header = [0u8; 20];
    std::fs::File::open(database).and_then(|mut file| file.read_exact(&mut header)).is_ok() && header[18..20] == [2, 2]
}
