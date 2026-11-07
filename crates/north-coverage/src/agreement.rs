use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::coverage_record::{self, Line};
use north_base::platform::land_queue;

use crate::map;
use crate::run_selected::{Ran, git, ran};

const USAGE: &str = "usage: north test coverage agreement --landings COUNT [--queue DIRECTORY]";
pub const SHORT: u8 = 1;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub landings: usize,
    pub rust: usize,
    pub selected: usize,
    pub misses: usize,
}

impl Tally {
    pub fn agrees(&self, wanted: usize) -> bool {
        self.landings >= wanted && self.rust > 0 && 2 * self.selected >= self.rust && self.misses == 0
    }
}

pub fn tally(lines: &[Line], newest: usize, candidate_log: impl Fn(&Path) -> Option<String>) -> Tally {
    let landed: Vec<(&str, &Path)> = lines.iter().filter_map(|line| if let Line::Landed { sha, log } = line { Some((sha.as_str(), log.as_path())) } else { None }).collect();
    let landed = &landed[landed.len().saturating_sub(newest)..];
    let shas: BTreeSet<&str> = landed.iter().map(|(sha, _)| *sha).collect();
    let runs: Vec<Ran> = landed.iter().filter_map(|(_, log)| candidate_log(log).as_deref().and_then(ran)).collect();
    let misses = lines.iter().filter_map(|line| if let Line::Judged { sha, misses } = line { shas.contains(sha.as_str()).then_some(misses) } else { None }).sum();
    Tally { landings: landed.len(), rust: runs.len(), selected: runs.iter().filter(|run| **run == Ran::Selected).count(), misses }
}

pub fn queue(root: &Path) -> Result<PathBuf, String> {
    let origin = git(root, &["config", "--get", "remote.origin.url"])?;
    Ok(land_queue::directory(&map::state_home(), &origin))
}

pub fn run(arguments: &[String]) -> u8 {
    let (mut wanted, mut queue_directory) = (None, None);
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--landings", Some(count)) if count.parse::<usize>().is_ok_and(|count| count > 0) => wanted = count.parse().ok(),
            ("--queue", Some(directory)) => queue_directory = Some(PathBuf::from(directory)),
            _ => return usage(),
        }
    }
    let Some(wanted) = wanted else { return usage() };
    let queue_directory = match queue_directory.map_or_else(|| queue(Path::new(".")), Ok) {
        Ok(directory) => directory,
        Err(why) => {
            eprintln!("north test coverage: cannot find the land queue: {why}");
            return 2;
        }
    };
    let path = coverage_record::path(&queue_directory);
    let lines = match coverage_record::read(&path) {
        Ok(lines) => lines,
        Err(error) => {
            eprintln!("north test coverage: cannot read {}: {error}", path.display());
            return 2;
        }
    };
    let tally = tally(&lines, wanted, |log| fs::read_to_string(log).ok());
    println!(
        "north test coverage: of the newest {} landings, {} ran the coverage check and {} of those ran a selection before the push instead of standing down; {} failed tests after them were skipped by their selection",
        tally.landings, tally.rust, tally.selected, tally.misses
    );
    if tally.agrees(wanted) {
        return 0;
    }
    if tally.landings < wanted {
        println!("north test coverage: {} landings are recorded, short of the {wanted} asked for", tally.landings);
    }
    SHORT
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}
