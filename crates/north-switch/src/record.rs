//! What `north fleet off` stopped, one line each in `$XDG_STATE_HOME/north/fleet-off`: `unit HOST NAME
//! transient|kept` or `freeze HOST`. A second `off` adds to it, and `north fleet on` starts what it lists and
//! keeps only the lines it could not put back.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::host::valid_name;
use crate::units::Unit;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Entry {
    Unit { host: String, name: String, transient: bool },
    Freeze { host: String },
}

impl Entry {
    pub fn unit(host: &str, unit: &Unit) -> Self {
        Self::Unit { host: host.to_owned(), name: unit.name.clone(), transient: unit.transient }
    }

    pub fn host(&self) -> &str {
        match self {
            Self::Unit { host, .. } | Self::Freeze { host } => host,
        }
    }

    fn line(&self) -> String {
        match self {
            Self::Unit { host, name, transient } => format!("unit {host} {name} {}", if *transient { "transient" } else { "kept" }),
            Self::Freeze { host } => format!("freeze {host}"),
        }
    }

    fn parse(line: &str) -> Option<Self> {
        match line.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["unit", host, name, kind] if valid_name(host) => Some(Self::Unit { host: (*host).to_owned(), name: (*name).to_owned(), transient: *kind == "transient" }),
            ["freeze", host] if valid_name(host) => Some(Self::Freeze { host: (*host).to_owned() }),
            _ => None,
        }
    }
}

pub fn path(state_home: &Path) -> PathBuf {
    state_home.join("north/fleet-off")
}

pub fn read(state_home: &Path) -> Result<Vec<Entry>, String> {
    let path = path(state_home);
    match fs::read_to_string(&path) {
        Ok(text) => text.lines().map(|line| Entry::parse(line).ok_or_else(|| format!("{}: not a line north fleet off writes: {line}", path.display()))).collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

pub fn write(state_home: &Path, entries: &[Entry]) -> Result<(), String> {
    let path = path(state_home);
    if entries.is_empty() {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {error}", path.display())),
            _ => Ok(()),
        };
    }
    let lines: BTreeSet<String> = entries.iter().map(Entry::line).collect();
    let text: String = lines.into_iter().map(|line| line + "\n").collect();
    let staged = path.with_extension("new");
    let parent = path.parent().unwrap_or(state_home);
    fs::create_dir_all(parent).and_then(|()| fs::write(&staged, text)).and_then(|()| fs::rename(&staged, &path)).map_err(|error| format!("{}: {error}", path.display()))
}
