use std::collections::HashSet;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use north_base::verdict_cache::path_set::{Entry, Kind};

use crate::fence::{LIBC_ETC_FILES, SYSTEM_ROOTS};

pub const TARGET_LABEL: &str = "target";

const DROPPED: &[&str] = &["/nix/store", "/dev/null", "/dev/zero", "/dev/urandom", "/proc/self", "/proc/thread-self"];

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
        if is_dropped(pid, path) || self.roots.program.as_deref() == Some(path) || self.is_scratch(path) {
            return Outcome::Drop;
        }
        let metadata = std::fs::symlink_metadata(path);
        let exists = metadata.is_ok();
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
        ambient(path, metadata)
    }

    fn tree_read(&self, role: Role, path: &Path, relative: &Path, metadata: &std::io::Result<Metadata>) -> Outcome {
        if relative.components().next().is_some_and(|first| first.as_os_str() == ".git") {
            return Outcome::Unreusable { reason: format!("read of the checked tree's .git: {}", path.display()), refused: metadata.is_ok() };
        }
        let name = if relative.as_os_str().is_empty() { Some(".") } else { relative.to_str() };
        let Some(name) = name.filter(|name| !name.contains(['\t', '\n'])) else {
            return Outcome::Unreusable { reason: format!("path is not a one-line UTF-8 name: {}", path.display()), refused: false };
        };
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

fn ambient(path: &Path, metadata: &std::io::Result<Metadata>) -> Outcome {
    Outcome::Unreusable { reason: format!("ambient path {}", path.display()), refused: metadata.is_ok() }
}

fn refused(reason: String) -> Outcome {
    Outcome::Unreusable { reason, refused: true }
}

fn is_dropped(pid: u32, path: &Path) -> bool {
    DROPPED.iter().chain(SYSTEM_ROOTS).any(|root| path.starts_with(root)) || LIBC_ETC_FILES.iter().any(|file| path == Path::new(file)) || path.starts_with(format!("/proc/{pid}"))
}

pub fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => result.push(part),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    result
}
