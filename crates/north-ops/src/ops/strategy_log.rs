//! `north strategy log`: reads the decision log back into its types (`check`) and keeps it to its
//! retention (`retain`): closed hours compressed with zstd, days past 14 deleted.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_strategy_types::{Envelope, Kind, Snapshot, admission, land, placement};
use serde::de::DeserializeOwned;

use crate::foundation::clock::Timestamp;
use crate::platform::decision_log;

const USAGE: &str = "usage: north strategy log check [--since DURATION]\n       north strategy log retain [--days N]";
const KEEP_DAYS: i64 = 14;

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let Some(root) = decision_log::root() else {
        eprintln!("north strategy log: the decision log is off (NORTH_DECISION_LOG is empty)");
        return 2;
    };
    let now = decision_log::now();
    match words.as_slice() {
        ["log", "check"] => check(&root, now - 3600.0, now),
        ["log", "check", "--since", since] => match decision_log::span_seconds(since) {
            Some(span) => check(&root, now - span, now),
            None => usage(),
        },
        ["log", "retain"] => retain(&root, now, KEEP_DAYS),
        ["log", "retain", "--days", days] => days.parse().map_or_else(|_| usage(), |days| retain(&root, now, days)),
        _ => usage(),
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

#[derive(Default)]
struct Tally {
    lines: usize,
    unparsed: Vec<String>,
    decide_us: Vec<u64>,
}

/// Parses every line from `since` on into its kind's types and prints one line per kind; exits 1 when
/// any line does not parse or an admission call names a snapshot no snapshot file holds.
fn check(root: &Path, since: f64, now: f64) -> u8 {
    let (snapshots, mut failed) = snapshot_hashes(root, since, now);
    let mut named = Vec::new();
    for kind in Kind::ALL {
        let mut tally = tally(root, kind, since, now, &mut named);
        failed |= !tally.unparsed.is_empty();
        println!("{}", summary(kind, &mut tally));
        for unparsed in tally.unparsed.iter().take(3) {
            println!("  does not parse: {unparsed}");
        }
    }
    let missing: Vec<&(PathBuf, String)> = named.iter().filter(|(_, hash)| !snapshots.contains(hash)).collect();
    if let Some((path, hash)) = missing.first() {
        failed = true;
        println!("admission: {} calls name a snapshot no snapshot file holds, first {hash} in {}", missing.len(), path.display());
    }
    u8::from(failed)
}

fn snapshot_hashes(root: &Path, since: f64, now: f64) -> (BTreeSet<String>, bool) {
    let mut failed = false;
    let mut hashes = BTreeSet::new();
    for (path, line) in decision_log::lines(root, Kind::Admission, "snapshots.jsonl", since, now) {
        match serde_json::from_str::<Snapshot<admission::Snapshot>>(&line) {
            Ok(snapshot) => {
                hashes.insert(snapshot.hash);
            }
            Err(error) => {
                failed = true;
                println!("admission snapshot: {} does not parse: {error}", path.display());
            }
        }
    }
    (hashes, failed)
}

/// The lines of one kind from `since` on, each parsed into its types; admission calls add the snapshot they name to `named`.
fn tally(root: &Path, kind: Kind, since: f64, now: f64, named: &mut Vec<(PathBuf, String)>) -> Tally {
    let mut tally = Tally::default();
    for (path, line) in decision_log::lines(root, kind, "jsonl", since, now) {
        let parsed = match kind {
            Kind::Placement => parse::<placement::Case, placement::Decision>(&line).map(|(at, us, _)| (at, us)),
            Kind::Admission => parse::<admission::Case, admission::Decision>(&line).map(|(at, us, case)| {
                named.push((path.clone(), case.snapshot));
                (at, us)
            }),
            Kind::LandPick => parse::<land::PickCase, land::Pick>(&line).map(|(at, us, _)| (at, us)),
            Kind::LandBatch => parse::<land::BatchCase, land::BatchDecision>(&line).map(|(at, us, _)| (at, us)),
            Kind::LandJudge => parse::<land::JudgeCase, land::Judgement>(&line).map(|(at, us, _)| (at, us)),
            Kind::LandWave => parse::<land::WaveCase, land::WaveDecision>(&line).map(|(at, us, _)| (at, us)),
            Kind::LandSheriff => parse::<land::SheriffCase, land::SheriffStep>(&line).map(|(at, us, _)| (at, us)),
        };
        match parsed {
            Ok((at, _)) if at < since => {}
            Ok((_, us)) => {
                tally.lines += 1;
                tally.decide_us.push(us);
            }
            Err(error) => {
                tally.lines += 1;
                tally.unparsed.push(format!("{}: {error}", path.display()));
            }
        }
    }
    tally
}

fn parse<C: DeserializeOwned, D: DeserializeOwned>(line: &str) -> Result<(f64, u64, C), String> {
    let envelope: Envelope<C, D> = serde_json::from_str(line).map_err(|error| error.to_string())?;
    Ok((envelope.at, envelope.decide_us, envelope.case))
}

fn summary(kind: Kind, tally: &mut Tally) -> String {
    tally.decide_us.sort_unstable();
    let timing = match (tally.decide_us.get(tally.decide_us.len() / 2), tally.decide_us.last()) {
        (Some(median), Some(most)) => format!(", decide_us median {median} max {most}"),
        _ => String::new(),
    };
    format!("{}: {} lines, {} unparsed{timing}", kind.name(), tally.lines, tally.unparsed.len())
}

/// Compresses each closed hour file and deletes each day directory older than `days`.
fn retain(root: &Path, now: f64, days: i64) -> u8 {
    let today = Timestamp::from_unix_seconds_f64(now).unix_days();
    let open_hour = decision_log::hour_file(Path::new(""), Kind::Placement, now, "jsonl").file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let open_day = Timestamp::from_unix_seconds_f64(now).iso_date();
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut failed = false;
    for kind in Kind::ALL {
        for day in entries(&root.join(kind.name())) {
            let name = day.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(age) = Timestamp::parse_loose(&format!("{name}T00:00:00Z")).map(|start| today - start.unix_days()) else { continue };
            if age > days {
                failed |= std::fs::remove_dir_all(&day).is_err();
                *tally.entry("days deleted").or_default() += 1;
                continue;
            }
            for file in entries(&day).into_iter().filter(|file| file.extension().is_some_and(|ext| ext == "jsonl")) {
                let hour = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                let open = name == open_day && (hour == open_hour || hour == open_hour.replace(".jsonl", ".snapshots.jsonl"));
                if open {
                    continue;
                }
                let packed = Command::new("zstd").args(["-q", "-f", "--rm", "-19"]).arg(&file).status().is_ok_and(|status| status.success());
                failed |= !packed;
                *tally.entry(if packed { "hours compressed" } else { "hours left uncompressed" }).or_default() += 1;
            }
        }
    }
    let shown: Vec<String> = tally.iter().map(|(what, count)| format!("{count} {what}")).collect();
    let _ = writeln!(std::io::stdout(), "north strategy log retain: {}", if shown.is_empty() { "nothing to do".to_owned() } else { shown.join(", ") });
    u8::from(failed)
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_line_parses_back_and_a_malformed_line_fails_the_check() {
        let root = tempfile::tempdir().unwrap();
        let at = 1_793_917_506.0;
        let file = decision_log::hour_file(root.path(), Kind::LandWave, at, "jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let good = Envelope { v: 1, kind: Kind::LandWave, id: "x".into(), at, host: "h".into(), strategy: "live:dev".into(), follows: None, case: land::WaveCase { wanted: 4, slots: Some((2, 6)) }, decision: land::WaveDecision { width: 2 }, decide_us: 3, dropped: 0 };
        std::fs::write(&file, format!("{}\n", serde_json::to_string(&good).unwrap())).unwrap();
        assert_eq!(check(root.path(), at - 60.0, at + 60.0), 0);
        std::fs::write(&file, format!("{}\n{{\"v\":1,\"kind\":\"land.wave\"}}\n", serde_json::to_string(&good).unwrap())).unwrap();
        assert_eq!(check(root.path(), at - 60.0, at + 60.0), 1);
    }
}
