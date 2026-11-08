use std::collections::HashSet;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

use north_base::verdict_cache::path_set::{Entry, Kind};

use crate::fence::{LIBC_ETC_FILES, SYSTEM_ROOTS};
use crate::not_inputs;
use crate::paths::PROC;

pub const TARGET_LABEL: &str = "target";

const DROPPED: &[&str] = &["/nix/store", "/dev/null", "/dev/zero", "/dev/urandom", "/dev/tty"];

const VOLATILE_PROC: &[&str] = &["loadavg", "meminfo", "stat", "uptime", "cpuinfo", "vmstat", "diskstats", "interrupts", "softirqs", "schedstat", "pressure", "sys/kernel/random"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Read,
    Probe,
    Exec,
    Listing,
    Watch,
    Open { write: bool, create: bool },
    MakeDir,
    Place,
    Write,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Drop,
    Record(Entry),
    Unreusable { reason: String, refused: bool },
}

pub struct Roots {
    pub tree: PathBuf,
    pub target: PathBuf,
    pub scratch: PathBuf,
    pub program: Option<PathBuf>,
    pub library_dirs: Vec<PathBuf>,
}

pub struct Classifier {
    roots: Roots,
    created: HashSet<PathBuf>,
}

impl Classifier {
    pub fn new(roots: Roots) -> Self {
        Self { roots, created: HashSet::new() }
    }

    pub fn classify(&mut self, pid: u32, role: Role, path: &Path) -> Outcome {
        if is_dropped(path) || self.roots.program.as_deref() == Some(path) || self.is_scratch(path) {
            return Outcome::Drop;
        }
        let metadata = std::fs::symlink_metadata(path);
        let exists = metadata.is_ok();
        if not_inputs::is_path(&self.roots.library_dirs, pid, path, metadata.as_ref().is_err_and(|error| error.kind() == ErrorKind::NotFound)) {
            return Outcome::Drop;
        }
        if let Some(outcome) = volatile(pid, path, exists) {
            return outcome;
        }
        match role {
            Role::MakeDir | Role::Open { create: true, .. } | Role::Place if !exists => {
                self.created.insert(path.to_owned());
                Outcome::Drop
            }
            Role::Place | Role::Write | Role::Open { write: true, .. } if exists => refused(format!("wrote {}", path.display())),
            Role::Open { .. } | Role::Place | Role::Write | Role::MakeDir => self.read(Role::Read, path, &metadata),
            Role::Watch | Role::Read | Role::Probe | Role::Exec | Role::Listing => self.read(role, path, &metadata),
        }
    }

    fn is_scratch(&self, path: &Path) -> bool {
        path.starts_with(&self.roots.scratch) || path.ancestors().any(|ancestor| self.created.contains(ancestor))
    }

    fn read(&self, role: Role, path: &Path, metadata: &std::io::Result<Metadata>) -> Outcome {
        if let Ok(relative) = path.strip_prefix(&self.roots.target) {
            return target_read(role, path, relative, metadata);
        }
        if let Ok(relative) = path.strip_prefix(&self.roots.tree) {
            return self.tree_read(role, path, relative, metadata);
        }
        if self.roots.tree.starts_with(path) && role != Role::Listing {
            return Outcome::Drop;
        }
        host_read(role, path, metadata)
    }

    fn tree_read(&self, role: Role, path: &Path, relative: &Path, metadata: &std::io::Result<Metadata>) -> Outcome {
        if relative.components().next().is_some_and(|first| first.as_os_str() == ".git") {
            return Outcome::Unreusable { reason: format!("read of the checked tree's .git: {}", path.display()), refused: metadata.is_ok() };
        }
        let name = if relative.as_os_str().is_empty() { Some(".") } else { relative.to_str() };
        let Some(name) = name.filter(|name| !name.contains(['\t', '\n'])) else {
            return Outcome::Unreusable { reason: format!("path is not a one-line UTF-8 name: {}", path.display()), refused: false };
        };
        entry(role, name, metadata)
    }
}

fn entry(role: Role, name: &str, metadata: &std::io::Result<Metadata>) -> Outcome {
    let record = |kind| Outcome::Record(Entry { kind, path: name.to_owned() });
    match (role, metadata) {
        (Role::Listing, Ok(found)) if found.is_dir() => record(Kind::Listing),
        (Role::Listing, _) => Outcome::Drop,
        (_, Err(error)) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => record(Kind::Absent),
        (_, Ok(found)) if found.is_dir() && role == Role::Watch => record(Kind::Listing),
        (_, Ok(found)) if found.is_dir() => Outcome::Drop,
        (Role::Exec, _) => record(Kind::Exec),
        _ => record(Kind::Content),
    }
}

fn target_read(role: Role, path: &Path, relative: &Path, metadata: &std::io::Result<Metadata>) -> Outcome {
    let unreusable = |what: &str| Outcome::Unreusable { reason: format!("{what}: {}", path.display()), refused: false };
    let Some(name) = relative.to_str().filter(|name| !name.contains(['\t', '\n'])) else {
        return unreusable("path under target is not a one-line UTF-8 name");
    };
    match (role, metadata) {
        (Role::Listing, _) => unreusable("listing of a directory under target"),
        (_, Err(_)) => unreusable("absent path under target"),
        (_, Ok(found)) if found.is_dir() => Outcome::Drop,
        (_, Ok(found)) if found.permissions().mode() & 0o111 != 0 => {
            let kind = if role == Role::Exec { Kind::Exec } else { Kind::Content };
            let path = if name.is_empty() { TARGET_LABEL.to_owned() } else { format!("{TARGET_LABEL}/{name}") };
            Outcome::Record(Entry { kind, path })
        }
        _ => unreusable("read of a file under target that is not an artifact"),
    }
}

fn host_read(role: Role, path: &Path, metadata: &std::io::Result<Metadata>) -> Outcome {
    let unreusable = |reason: String| Outcome::Unreusable { reason, refused: metadata.is_ok() };
    if path.starts_with("/tmp") && (path != Path::new("/tmp") || role == Role::Listing) {
        return unreusable(format!("read of a /tmp path that is neither scratch nor made by the unit: {}", path.display()));
    }
    if metadata.as_ref().is_ok_and(|found| {
        let kind = found.file_type();
        kind.is_char_device() || kind.is_block_device() || kind.is_fifo() || kind.is_socket()
    }) {
        return unreusable(format!("read of a device or special file: {}", path.display()));
    }
    match path.to_str().filter(|name| !name.contains(['\t', '\n'])) {
        Some(name) => entry(role, name, metadata),
        None => Outcome::Unreusable { reason: format!("path is not a one-line UTF-8 name: {}", path.display()), refused: false },
    }
}

fn volatile(pid: u32, path: &Path, exists: bool) -> Option<Outcome> {
    let rest = path.strip_prefix(PROC).ok()?;
    let own = not_inputs::own_proc(pid, path);
    let is_process = rest.components().next().is_some_and(|first| first.as_os_str().to_str().is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()) || name == "self" || name == "thread-self"));
    let is_host_volatile = VOLATILE_PROC.iter().any(|name| rest.starts_with(name));
    let nothing_to_read = own.is_some_and(|inner| inner.as_os_str().is_empty());
    (!nothing_to_read && (is_process || is_host_volatile)).then(|| Outcome::Unreusable { reason: format!("volatile read: {}", path.display()), refused: exists })
}

fn refused(reason: String) -> Outcome {
    Outcome::Unreusable { reason, refused: true }
}

fn is_dropped(path: &Path) -> bool {
    DROPPED.iter().chain(SYSTEM_ROOTS).any(|root| path.starts_with(root)) || LIBC_ETC_FILES.iter().any(|file| path == Path::new(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::proc;

    const PID: u32 = 4242;

    fn reason(outcome: Option<Outcome>) -> Option<String> {
        match outcome {
            Some(Outcome::Unreusable { reason, .. }) => Some(reason),
            _ => None,
        }
    }

    #[test]
    fn a_volatile_proc_read_is_unreusable_naming_the_path_and_the_process_own_stable_state_is_not() {
        for rest in ["loadavg", "meminfo", "uptime", "pressure/cpu", "self/status", "4242/smaps", "4243/status", "sys/kernel/random/uuid"] {
            let path = proc(rest);
            assert_eq!(reason(volatile(PID, &path, true)), Some(format!("volatile read: {}", path.display())));
        }
        for rest in ["self", "4242", "sys/kernel/ostype", "version"] {
            assert!(volatile(PID, &proc(rest), true).is_none(), "{rest}");
        }
    }

    #[test]
    fn the_parallelism_probes_and_the_process_own_links_are_not_inputs_and_other_proc_files_are() {
        let absent = false;
        for rest in ["self/cgroup", "self/mountinfo", "4242/cgroup", "thread-self/mountinfo", "self/exe", "self/fd/3", "self/maps"] {
            assert!(not_inputs::is_path(&[], PID, &proc(rest), absent), "{rest}");
        }
        for rest in ["4243/cgroup", "4243/maps", "self/status"] {
            assert!(!not_inputs::is_path(&[], PID, &proc(rest), absent), "{rest}");
        }
        assert!(not_inputs::is_path(&[], PID, Path::new("/sys/fs/cgroup/user.slice/cpu.max"), absent));
        for path in ["/sys/fs/cgroup/user.slice/memory.max", "/etc/hosts"] {
            assert!(!not_inputs::is_path(&[], PID, Path::new(path), absent), "{path}");
        }
        assert!(not_inputs::is_call("sched_getaffinity") && !not_inputs::is_call("statfs"));
    }

    #[test]
    fn only_an_absent_library_probe_under_a_library_path_directory_is_not_an_input() {
        let dirs = not_inputs::library_dirs(Some("/work/target/debug/deps:relative/ignored".into()));
        let probe = |path: &str, absent| not_inputs::is_path(&dirs, PID, Path::new(path), absent);
        assert!(probe("/work/target/debug/deps/glibc-hwcaps/x86-64-v3/libc.so.6", true));
        assert!(probe("/work/target/debug/deps/libc.so.6", true));
        assert!(!probe("/work/target/debug/deps/libc.so.6", false));
        assert!(!probe("/work/target/debug/deps/notes.txt", true));
        assert!(!probe("/work/target/release/libc.so.6", true));
    }
}
