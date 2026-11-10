use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use north_base::verdict_cache::path_set::{self, Kind};

use crate::facts::{Facts, Sourced, text};
use crate::places::Places;

const WRITER: &str = "recorder";
const REPLACED: [&str; 6] = ["reads", "lists", "expects-absent", "read-set", "recorded-in", "reads-commit"];

struct Unit<'a> {
    epoch: f64,
    probe: &'a str,
    set: &'a str,
}

pub fn read(places: &Places) -> Result<Sourced, String> {
    let mut facts = Facts::new(&REPLACED);
    let logs = match std::fs::read_dir(&places.land_reuse) {
        Ok(entries) => entries,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            facts.note(format!("no shadow log at {}, so no test's reads are known", places.land_reuse.display()));
            return Ok(facts.finish_noted());
        }
        Err(why) => return Err(format!("{}: {why}", places.land_reuse.display())),
    };
    let mut paths: Vec<PathBuf> = logs.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "tsv")).collect();
    paths.sort();
    let bodies = paths.iter().map(|path| std::fs::read_to_string(path).map_err(|why| format!("{}: {why}", path.display()))).collect::<Result<Vec<_>, _>>()?;
    let mut newest: BTreeMap<(&str, &str), Unit> = BTreeMap::new();
    let mut probe: Option<(f64, &str)> = None;
    for fields in bodies.iter().flat_map(|body| body.lines()).map(|line| line.split('\t').collect::<Vec<_>>()) {
        let (epoch, line_probe, unit, set) = match fields.as_slice() {
            [epoch, "unit", probe, unit, set, "pass" | "fail", _] | [epoch, "recorded", probe, unit, set] => (epoch, probe, unit, set),
            _ => continue,
        };
        let (Ok(epoch), Some((binary, name))) = (epoch.parse::<f64>(), unit.split_once(' ')) else { continue };
        if probe.is_none_or(|(at, _)| epoch >= at) {
            probe = Some((epoch, line_probe));
        }
        if *set != "-" && newest.get(&(binary, name)).is_none_or(|known| epoch >= known.epoch) {
            newest.insert((binary, name), Unit { epoch, probe: line_probe, set });
        }
    }
    if let Some((_, probe)) = probe {
        facts.tell(WRITER, text("reads:north"), "reads-commit", text(probe));
    }
    let found = set_paths(&places.verdict_cache, &newest.values().map(|unit| unit.set).collect());
    let mut unfound = 0;
    for ((binary, name), unit) in &newest {
        let Some(body) = found.get(unit.set).and_then(|path| std::fs::read_to_string(path).ok()) else {
            unfound += 1;
            continue;
        };
        let entries = path_set::parse(&body).map_err(|why| format!("set {} of {binary} {name} at {}: {why}", unit.set, unit.probe))?;
        let test = format!("test:{binary}::{name}");
        facts.tell(WRITER, text(&test), "read-set", text(unit.set));
        facts.tell(WRITER, text(&test), "recorded-in", text(*binary));
        for entry in entries.iter().filter(|entry| !(entry.path == "target" || entry.path.starts_with("target/"))) {
            let predicate = match entry.kind {
                Kind::Content | Kind::Exec => "reads",
                Kind::Listing => "lists",
                Kind::Absent => "expects-absent",
                Kind::Unreusable => continue,
            };
            facts.tell(WRITER, text(&test), predicate, text(&entry.path));
        }
    }
    if unfound > 0 {
        facts.note(format!("{unfound} tests name a path set {} does not hold", places.verdict_cache.display()));
    }
    Ok(facts.finish_noted())
}

/// Where each wanted path set lies, found in one pass over the cache: a set's id is its content's hash, so any weak key's copy serves, and listing the whole cache once per test made one ingest take minutes of system time.
fn set_paths(verdict_cache: &Path, wanted: &BTreeSet<&str>) -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    for weak in std::fs::read_dir(verdict_cache.join("v2")).into_iter().flatten().flatten() {
        for set in std::fs::read_dir(weak.path().join("sets")).into_iter().flatten().flatten() {
            if let Some(id) = set.file_name().to_str().filter(|id| wanted.contains(id)) {
                found.entry(id.to_owned()).or_insert_with(|| set.path());
            }
        }
    }
    found
}
