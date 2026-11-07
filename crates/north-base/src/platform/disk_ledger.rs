//! The host's disk ledger: every store north or a project writes on the root disk, from `harness/disk-ledger.toml`, with its kind, its
//! trim and its budget, and an account of the bytes each holds beside the bytes the disk uses that no store explains.
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use super::disk_budget::DiskBudget;

pub const PATH: &str = "harness/disk-ledger.toml";
const GIB: f64 = (1u64 << 30) as f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cache,
    Warm,
    Lane,
    State,
}

impl Kind {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "cache" => Some(Self::Cache),
            "warm" => Some(Self::Warm),
            "lane" => Some(Self::Lane),
            "state" => Some(Self::State),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Warm => "warm",
            Self::Lane => "lane",
            Self::State => "state",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: Kind,
    pub trim: String,
}

impl Entry {
    /// The directories this entry names under `home`, one `*` per segment matching any run of characters there.
    pub fn resolve(&self, home: &Path) -> Vec<PathBuf> {
        let (mut found, rest) = match self.path.strip_prefix('/') {
            Some(rest) => (vec![PathBuf::from("/")], rest),
            None => (vec![home.to_path_buf()], self.path.as_str()),
        };
        for segment in rest.split('/').filter(|segment| !segment.is_empty()) {
            found = found.iter().flat_map(|parent| matches(parent, segment)).collect();
        }
        found.retain(|path| path.is_dir());
        found.sort();
        found
    }
}

fn matches(parent: &Path, segment: &str) -> Vec<PathBuf> {
    let Some((head, tail)) = segment.split_once('*') else { return vec![parent.join(segment)] };
    let Ok(entries) = fs::read_dir(parent) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(|name| name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)))
        .map(|entry| entry.path())
        .collect()
}

pub struct Ledger {
    pub entries: Vec<Entry>,
}

impl Ledger {
    pub fn parse(text: &str) -> Result<Self, String> {
        let document: DocumentMut = text.parse().map_err(|error| format!("{PATH}: {error}"))?;
        let stores = document.get("store").and_then(|item| item.as_array_of_tables()).ok_or_else(|| format!("{PATH}: no [[store]] entries"))?;
        let mut entries = Vec::new();
        for store in stores {
            let field = |key: &str| store.get(key).and_then(|value| value.as_str()).map(str::to_owned).ok_or_else(|| format!("{PATH}: a store has no {key}"));
            let name = field("name")?;
            let kind = Kind::parse(&field("kind")?).ok_or_else(|| format!("{PATH}: store {name} has a kind other than cache, warm, lane or state"))?;
            if entries.iter().any(|entry: &Entry| entry.name == name) {
                return Err(format!("{PATH}: store {name} is listed twice"));
            }
            entries.push(Entry { path: field("path")?, trim: field("trim")?, kind, name });
        }
        Ok(Self { entries })
    }

    pub fn shipped() -> Self {
        Self::parse(super::harness::table(PATH)).unwrap_or_else(|error| panic!("north: {error}"))
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.name == name)
    }
}

pub struct Row {
    pub entry: Entry,
    pub bytes: u64,
    pub budget_gib: Option<f64>,
}

/// The bytes each store holds on the root disk, a store nested in another counted only in itself, and the used bytes none explains.
pub struct Account {
    pub rows: Vec<Row>,
    pub used: u64,
    pub unregistered: Vec<(PathBuf, u64)>,
}

impl Account {
    pub fn registered(&self) -> u64 {
        self.rows.iter().map(|row| row.bytes).sum()
    }

    pub fn unaccounted(&self) -> u64 {
        self.used.saturating_sub(self.registered())
    }

    /// The `count` stores holding the most bytes, largest first.
    pub fn top(&self, count: usize) -> Vec<&Row> {
        let mut rows: Vec<&Row> = self.rows.iter().collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.bytes));
        rows.truncate(count);
        rows
    }

    pub fn lines(&self) -> Vec<String> {
        let gib = |bytes: u64| bytes as f64 / GIB;
        let mut lines: Vec<String> = self
            .rows
            .iter()
            .map(|row| {
                let budget = row.budget_gib.map_or(String::new(), |budget| format!(" of {budget:.0} GiB budget"));
                format!("{:>8.1} GiB  {:<5}  {:<22} {}{budget}  trim: {}", gib(row.bytes), row.entry.kind.word(), row.entry.name, row.entry.path, row.entry.trim)
            })
            .collect();
        lines.extend(self.unregistered.iter().map(|(path, bytes)| format!("{:>8.1} GiB  unregistered {}", gib(*bytes), path.display())));
        let share = if self.used == 0 { 0.0 } else { self.unaccounted() as f64 * 100.0 / self.used as f64 };
        lines.push(format!("{:>8.1} GiB  used; {:.1} GiB in stores; {:.1} GiB ({share:.1}%) unaccounted", gib(self.used), gib(self.registered()), gib(self.unaccounted())));
        lines
    }
}

/// Measures every store of `ledger` under `home` on the filesystem holding `home`, whose used bytes are `used`.
pub fn account(ledger: &Ledger, home: &Path, used: u64, budget: &DiskBudget) -> Account {
    let Ok(device) = fs::metadata(home).map(|metadata| metadata.dev()) else { return Account { rows: Vec::new(), used, unregistered: Vec::new() } };
    let disk = source(device);
    let on_disk = |other: u64| other == device || (disk.is_some() && source(other) == disk);
    let resolved: Vec<(usize, PathBuf)> = ledger.entries.iter().enumerate().flat_map(|(index, entry)| entry.resolve(home).into_iter().map(move |path| (index, path))).collect();
    let all: HashSet<PathBuf> = resolved.iter().map(|(_, path)| path.clone()).collect();
    let mut seen = HashSet::new();
    let mut bytes = vec![0u64; ledger.entries.len()];
    for (index, path) in &resolved {
        let Ok(root) = fs::metadata(path).map(|metadata| metadata.dev()) else { continue };
        if on_disk(root) {
            bytes[*index] += walk(path, root, &all, &mut seen, true);
        }
    }
    let unregistered = unregistered(home, device, &all, &mut seen);
    let rows = ledger.entries.iter().zip(bytes).map(|(entry, bytes)| Row { entry: entry.clone(), bytes, budget_gib: budget.named_gib(&entry.name) }).collect();
    Account { rows, used, unregistered }
}

/// Each child of the home that no store is, holds or sits in, with its size, largest first.
fn unregistered(home: &Path, device: u64, stores: &HashSet<PathBuf>, seen: &mut HashSet<(u64, u64)>) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(home) else { return Vec::new() };
    let mut found: Vec<(PathBuf, u64)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| !stores.iter().any(|store| store.starts_with(path) || path.starts_with(store)))
        .map(|path| {
            let bytes = walk(&path, device, stores, seen, true);
            (path, bytes)
        })
        .filter(|(_, bytes)| *bytes > 0)
        .collect();
    found.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    found
}

fn walk(path: &Path, device: u64, stores: &HashSet<PathBuf>, seen: &mut HashSet<(u64, u64)>, top: bool) -> u64 {
    if !top && stores.contains(path) {
        return 0;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else { return 0 };
    if metadata.dev() != device {
        return 0;
    }
    if metadata.nlink() > 1 && !metadata.is_dir() && !seen.insert((metadata.dev(), metadata.ino())) {
        return 0;
    }
    let own = metadata.blocks() * 512;
    if !metadata.is_dir() {
        return own;
    }
    let Ok(entries) = fs::read_dir(path) else { return own };
    own + entries.flatten().map(|entry| walk(&entry.path(), device, stores, seen, false)).sum::<u64>()
}

/// The mount source of the filesystem whose device is `device`, so the subvolumes of one btrfs disk, each its own device, count as one disk.
fn source(device: u64) -> Option<String> {
    let id = format!("{}:{}", rustix::fs::major(device), rustix::fs::minor(device));
    let mounts = fs::read_to_string("/proc/self/mountinfo").ok()?;
    mounts.lines().filter(|line| line.split(' ').nth(2) == Some(id.as_str())).find_map(|line| line.split_once(" - ")?.1.split(' ').nth(1).map(str::to_owned))
}

/// The bytes the filesystem holding `path` uses, as `df` counts them.
pub fn used_bytes(path: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some(stats.f_blocks.saturating_sub(stats.f_bfree).saturating_mul(stats.f_frsize))
}

/// The account of this host's home with the shipped ledger and this host's budget.
pub fn local() -> Option<Account> {
    let home = PathBuf::from(std::env::var_os("HOME").filter(|home| !home.is_empty())?);
    let used = used_bytes(&home)?;
    Some(account(&Ledger::shipped(), &home, used, &DiskBudget::local()))
}
