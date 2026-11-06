//! The scoreboard as a weekly loop: the fleet tick records it once a week and tells the proxy, once
//! per recording, which numbers got worse than the week before.
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Better, Measure, Sources, WEEK_S, measure, state_root};
use crate::foundation::clock::Timestamp;

const RECORD: &str = "north/scoreboard/week.json";
const SCORE: &str = "north/scoreboard/week.txt";
const TOLD: &str = "north/scoreboard/told.txt";
const MIN_SAMPLE: usize = 5;
const MARGIN: f64 = 0.10;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Number {
    pub name: String,
    pub better: Better,
    pub now: Option<f64>,
    pub before: Option<f64>,
    pub worse: Option<bool>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub recorded_at: i64,
    pub numbers: Vec<Number>,
}

fn worse(better: Better, now: f64, before: f64) -> bool {
    match better {
        Better::Lower => now > before * (1.0 + MARGIN),
        Better::Higher => now < before * (1.0 - MARGIN),
    }
}

impl Record {
    pub fn of(measures: &[Measure], recorded_at: i64) -> Self {
        let numbers = measures
            .iter()
            .map(|measure| {
                let [now, before] = measure.weeks.each_ref().map(|figure| figure.headline());
                let enough = |headline: &Option<(f64, usize)>| headline.filter(|(_, sample)| *sample >= MIN_SAMPLE).map(|(value, _)| value);
                let worse = enough(&now).zip(enough(&before)).map(|(now, before)| worse(measure.better, now, before));
                Number { name: measure.name.clone(), better: measure.better, now: now.map(|(value, _)| value), before: before.map(|(value, _)| value), worse }
            })
            .collect();
        Self { recorded_at, numbers }
    }

    pub fn worsened(&self) -> Vec<&str> {
        self.numbers.iter().filter(|number| number.worse == Some(true)).map(|number| number.name.as_str()).collect()
    }

    pub fn score(&self) -> String {
        let compared = self.numbers.iter().filter(|number| number.worse.is_some()).count();
        match self.worsened().as_slice() {
            [] => format!("scoreboard: none of {compared} compared numbers is worse than the week before"),
            worsened => format!("scoreboard: {} of {compared} compared numbers worse than the week before: {}", worsened.len(), worsened.join("; ")),
        }
    }
}

fn file(relative: &str) -> PathBuf {
    state_root().join(relative)
}

fn write(relative: &str, text: &str) -> Result<(), String> {
    let path = file(relative);
    let parent = path.parent().ok_or_else(|| format!("{} has no parent", path.display()))?;
    fs::create_dir_all(parent).and_then(|()| fs::write(&path, text)).map_err(|why| format!("{}: {why}", path.display()))
}

pub fn record(sources: &Sources, now: i64) -> Result<(Record, Vec<Measure>), String> {
    let measures = measure(sources, now)?;
    let record = Record::of(&measures, now);
    write(RECORD, &serde_json::to_string(&record).map_err(|why| why.to_string())?)?;
    write(SCORE, &format!("{}\n", record.score()))?;
    Ok((record, measures))
}

fn stored() -> Option<Record> {
    serde_json::from_str(&fs::read_to_string(file(RECORD)).ok()?).ok()
}

/// What the proxy's fleet tick says: the record is refreshed when a week old, and the numbers that got
/// worse are told once per recording.
pub fn tick(now: i64) -> Vec<String> {
    let current = stored().filter(|record| (0..WEEK_S).contains(&(now - record.recorded_at)));
    let record = match current {
        Some(record) => record,
        None => match self::record(&Sources::live(), now) {
            Ok((record, _)) => record,
            Err(why) => return vec![format!("scoreboard not recorded: {why}")],
        },
    };
    let told = fs::read_to_string(file(TOLD)).ok().and_then(|text| text.trim().parse::<i64>().ok());
    if record.worsened().is_empty() || told == Some(record.recorded_at) {
        return Vec::new();
    }
    let _ = write(TOLD, &record.recorded_at.to_string());
    vec![format!("{} (recorded {}; north scoreboard prints the figures)", record.score(), Timestamp::from_unix_seconds(record.recorded_at).local_twelve_hour())]
}

#[cfg(test)]
#[path = "weekly_tests.rs"]
mod tests;
