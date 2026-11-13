//! `north test coverage take [--repo NAME]`: installs a record's output on this host. The output is the map line and
//! one read line per test, as `north test coverage record` prints them; the refresh writes it beside the map and
//! sends it to every other host, so each host's graph gains the record's read sets as well as its map (north#1247).
use std::io::BufRead;
use std::path::{Path, PathBuf};

use north_base::verdict_cache::{Store, reuse_log};

use crate::map::{self, Map};
use crate::reads::{self, Read};
use crate::record::MAP_LINE;
use crate::run_selected::repository;

pub struct Recorded {
    pub map: Map,
    pub reads: Vec<Read>,
}

pub fn path_for(repository: &str) -> PathBuf {
    map::path_for(repository).with_extension("recorded")
}

impl Recorded {
    /// The last map in `lines` and the read lines printed after it; every other line goes to `other`.
    pub fn parse(lines: impl Iterator<Item = std::io::Result<String>>, mut other: impl FnMut(&str)) -> Result<Option<Self>, String> {
        let mut recorded: Option<Self> = None;
        for line in lines {
            let line = line.map_err(|error| format!("cannot read the record's output: {error}"))?;
            if let Some(text) = line.strip_prefix(MAP_LINE) {
                let map = serde_json::from_str::<Map>(text).map_err(|error| format!("cannot parse the recorded map: {error}"))?;
                recorded = Some(Self { map, reads: Vec::new() });
            } else if let Some(read) = Read::from_line(&line) {
                if let Some(recorded) = recorded.as_mut() {
                    recorded.reads.push(read);
                }
            } else {
                other(&line);
            }
        }
        Ok(recorded)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let map = serde_json::to_string(&self.map).map_err(|error| format!("cannot encode the coverage map: {error}"))?;
        let text: String = std::iter::once(format!("{MAP_LINE}{map}")).chain(self.reads.iter().filter_map(Read::line)).map(|line| line + "\n").collect();
        let directory = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
        std::fs::create_dir_all(directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let staged = tempfile::NamedTempFile::new_in(directory).map_err(|error| format!("cannot stage {}: {error}", path.display()))?;
        std::fs::write(staged.path(), text).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        staged.persist(path).map(|_| ()).map_err(|error| format!("cannot install {}: {error}", path.display()))
    }

    /// Stores each read set in this host's verdict cache, appends its recorded line to this host's reuse log and
    /// installs the map as `repository`'s, for the next `north graph ingest coverage reads`.
    pub fn install(&self, repository: &str) -> Result<PathBuf, String> {
        reuse_log::append(&reads::store(&Store::here(), &self.map.commit, &self.reads), &reuse_log::directory(&map::state_home()));
        let path = map::path_for(repository);
        self.map.save(&path)?;
        Ok(path)
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let repository = match arguments {
        [] => repository(Path::new(".")),
        [flag, name] if flag == "--repo" && crate::hosts::plain(name) => name.clone(),
        _ => {
            eprintln!("usage: north test coverage take [--repo NAME]");
            return 2;
        }
    };
    let path = path_for(&repository);
    let taken = std::fs::File::open(&path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))
        .and_then(|file| Recorded::parse(std::io::BufReader::new(file).lines(), |_| {}))
        .and_then(|recorded| recorded.ok_or_else(|| format!("{} holds no map", path.display())))
        .and_then(|recorded| recorded.install(&repository).map(|_| recorded.reads.len()));
    match taken {
        Ok(reads) => {
            println!("north test coverage take: installed {repository}'s map and {reads} read sets from {}", path.display());
            u8::from(!crate::cadence::ingest_here())
        }
        Err(why) => {
            eprintln!("north test coverage take: {why}");
            1
        }
    }
}
