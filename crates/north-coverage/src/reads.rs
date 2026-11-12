use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use north_base::verdict_cache::reuse_log::Record;
use north_base::verdict_cache::fingerprint::unit_id;
use north_base::verdict_cache::path_set;
use north_base::verdict_cache::{Store, set_id};

use crate::exec::Reached;

const WEAK: &str = "coverage-record";

pub fn probe(commit: &str) -> String {
    format!("coverage-{}", &commit[..commit.len().min(12)])
}

pub fn records(commit: &str, units: &BTreeMap<String, PathBuf>, binaries: &HashMap<String, String>, reached: &[Reached]) -> Vec<Record> {
    let store = Store::here();
    let probe = probe(commit);
    reached
        .iter()
        .filter_map(|reach| {
            let binary = binaries.get(&reach.binary)?;
            let bytes = std::fs::read(units.get(&reach.key)?).ok()?;
            path_set::parse(&String::from_utf8_lossy(&bytes)).ok()?;
            let unit = unit_id(binary, &reach.test);
            let set = store.record_set(&set_id(format!("{WEAK}\0{unit}").as_bytes()), &bytes).ok()?;
            Some(Record::Recorded { probe: probe.clone(), unit, set })
        })
        .collect()
}
