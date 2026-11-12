use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use north_land_scans::slug::slug_of;

use super::fetch_base;
use crate::land::candidate::git;
use crate::land::scans::outgoing::denylist::{Denylist, Held};

static KEPT: Mutex<Option<Facts>> = Mutex::new(None);

pub struct Repository {
    pub name: String,
    pub archived: bool,
    pub visibility: String,
}

type Base = Result<(String, String), String>;

struct Started<T> {
    handle: Mutex<Option<JoinHandle<T>>>,
    value: OnceLock<T>,
}

impl<T: Send + 'static> Started<T> {
    fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self { handle: Mutex::new(Some(std::thread::spawn(work))), value: OnceLock::new() }
    }

    fn get(&self, panicked: impl FnOnce() -> T) -> &T {
        self.value.get_or_init(|| match self.handle.lock().ok().and_then(|mut handle| handle.take()) {
            Some(handle) => handle.join().unwrap_or_else(|_| panicked()),
            None => panicked(),
        })
    }
}

struct Inner {
    root: PathBuf,
    origin: String,
    destination: String,
    base: Started<Base>,
    repository: Started<Repository>,
    denylist: Started<Option<Held>>,
}

/// The facts one hand-in needs more than once, each fetched once from the call's first instant: the destination branch, what the git host says of the repository, and the deny-list copy.
#[derive(Clone)]
pub struct Facts(Arc<Inner>);

impl Facts {
    pub fn start(repo: &Path, destination: &str, denylist: &Denylist) -> Option<Self> {
        let root = PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"]).ok()?);
        let origin = git(&root, &["remote", "get-url", "--push", "origin"]).ok()?;
        Some(Self::started(root, origin, destination.to_owned(), denylist))
    }

    pub(super) fn started(root: PathBuf, origin: String, destination: String, denylist: &Denylist) -> Self {
        let (fetching, fetched) = (root.clone(), destination.clone());
        let (looked_up, scanned, sources) = (origin.clone(), origin.clone(), denylist.clone());
        let asked = origin.contains("github.com") || denylist.scans();
        Self(Arc::new(Inner {
            base: Started::spawn(move || fetch_base(&fetching, &fetched)),
            repository: Started::spawn(move || if asked { lookup(&looked_up) } else { unknown(String::new()) }),
            denylist: Started::spawn(move || sources.prefetch(&scanned)),
            root,
            origin,
            destination,
        }))
    }

    pub fn keep(&self) {
        if let Ok(mut kept) = KEPT.lock() {
            *kept = Some(self.clone());
        }
    }

    pub fn kept() -> Option<Self> {
        KEPT.lock().ok().and_then(|kept| kept.clone())
    }

    pub(super) fn serves(&self, root: &Path, origin: &str, destination: &str) -> bool {
        self.0.root == root && self.0.origin == origin && self.0.destination == destination
    }

    pub fn base(&self) -> Base {
        self.0.base.get(|| Err("the destination fetch panicked".to_owned())).clone()
    }

    pub fn repository(&self) -> &Repository {
        self.0.repository.get(|| unknown(String::new()))
    }

    pub fn denylist(&self) -> Option<&Held> {
        self.0.denylist.get(|| None).as_ref()
    }
}

fn unknown(name: String) -> Repository {
    Repository { name, archived: false, visibility: "UNKNOWN".to_owned() }
}

fn lookup(origin: &str) -> Repository {
    let slug = origin.contains("github.com").then(|| slug_of(origin)).flatten();
    let answer = Command::new("timeout").args(["10", "gh", "repo", "view", origin, "--json", "isArchived,visibility", "-q", r#""\(.isArchived) \(.visibility)""#]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let name = slug.clone().unwrap_or_default();
    let Some(output) = answer.ok().filter(|output| output.status.success()) else { return unknown(name) };
    let text = String::from_utf8_lossy(&output.stdout);
    match text.split_whitespace().collect::<Vec<_>>()[..] {
        [archived, visibility] => Repository { name, archived: slug.is_some() && archived == "true", visibility: visibility.to_ascii_uppercase() },
        _ => unknown(name),
    }
}
