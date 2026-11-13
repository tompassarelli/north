use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use north_base::verdict_cache::fingerprint::unit_id;
use north_base::verdict_cache::path_set;
use north_base::verdict_cache::reuse_log::Record;
use north_base::verdict_cache::{Store, set_id};

use crate::exec::Reached;

pub const READ_LINE: &str = "north-coverage-read ";
const WEAK: &str = "coverage-record";

#[derive(Serialize, Deserialize)]
pub struct Read {
    pub unit: String,
    pub set: String,
}

impl Read {
    pub fn line(&self) -> Option<String> {
        serde_json::to_string(self).ok().map(|text| format!("{READ_LINE}{text}"))
    }

    pub fn from_line(line: &str) -> Option<Self> {
        serde_json::from_str(line.strip_prefix(READ_LINE)?).ok()
    }
}

pub fn probe(commit: &str) -> String {
    format!("coverage-{}", &commit[..commit.len().min(12)])
}

pub fn read(units: &BTreeMap<String, PathBuf>, binaries: &HashMap<String, String>, reached: &[Reached]) -> Vec<Read> {
    reached
        .iter()
        .filter_map(|reach| {
            let binary = binaries.get(&reach.binary)?;
            let set = String::from_utf8(std::fs::read(units.get(&reach.key)?).ok()?).ok()?;
            path_set::parse(&set).ok()?;
            Some(Read { unit: unit_id(binary, &reach.test), set })
        })
        .collect()
}

pub fn store(store: &Store, commit: &str, reads: &[Read]) -> Vec<Record> {
    let probe = probe(commit);
    reads
        .iter()
        .filter_map(|read| {
            let set = store.record_set(&set_id(format!("{WEAK}\0{}", read.unit).as_bytes()), read.set.as_bytes()).ok()?;
            Some(Record::Recorded { probe: probe.clone(), unit: read.unit.clone(), set })
        })
        .collect()
}
