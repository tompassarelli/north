//! The scoreboard as a weekly loop: the fleet tick records it once a week and tells the proxy, once
//! per recording, which numbers got worse than the week before. A recording is one transaction to north's
//! graph (north-graph-loops, writer `scoreboard`), and the week-on-week comparison reads it back through
//! harness/graph-rules/scoreboard.json.
use std::fs;
use std::path::PathBuf;

use north_graph_loops::scoreboard::{self, Reading, Told};
use time::OffsetDateTime;

use super::{Better, Measure, Sources, WEEK_S, measure, state_root};
use crate::foundation::clock::Timestamp;

const SCORE: &str = "north/scoreboard/week.txt";
const TOLD: &str = "north/scoreboard/told.txt";
const MIN_SAMPLE: usize = 5;
const MARGIN: f64 = 0.10;

type Headline = (f64, usize);

#[derive(Debug, PartialEq)]
pub struct Number {
    pub name: String,
    pub better: Better,
    pub now: Option<Headline>,
    pub before: Option<Headline>,
}

#[derive(Debug, PartialEq)]
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

fn week_label(at: i64) -> String {
    let (year, week, _) = OffsetDateTime::from_unix_timestamp(at).map_or((1970, 1, time::Weekday::Thursday), |date| date.date().to_iso_week_date());
    format!("{year}-W{week:02}")
}

impl Number {
    pub fn worse(&self) -> Option<bool> {
        let enough = |headline: &Option<Headline>| headline.filter(|(_, sample)| *sample >= MIN_SAMPLE).map(|(value, _)| value);
        enough(&self.now).zip(enough(&self.before)).map(|(now, before)| worse(self.better, now, before))
    }
}

impl Record {
    pub fn of(measures: &[Measure], recorded_at: i64) -> Self {
        let numbers = measures
            .iter()
            .map(|measure| {
                let [now, before] = measure.weeks.each_ref().map(|figure| figure.headline());
                Number { name: measure.name.clone(), better: measure.better, now, before }
            })
            .collect();
        Self { recorded_at, numbers }
    }

    fn readings(&self) -> Vec<Reading> {
        let weeks = [week_label(self.recorded_at), week_label(self.recorded_at - WEEK_S)];
        self.numbers
            .iter()
            .flat_map(|number| {
                [number.now, number.before].into_iter().zip(weeks.clone()).filter_map(|(headline, week)| {
                    let (figure, observations) = headline?;
                    Some(Reading { number: number.name.clone(), week, figure, observations, higher_is_better: number.better == Better::Higher })
                })
            })
            .collect()
    }

    fn told(told: &Told) -> Option<Self> {
        let recorded_at = told.at?;
        let weeks = [week_label(recorded_at), week_label(recorded_at - WEEK_S)];
        let mut numbers: Vec<Number> = Vec::new();
        for reading in &told.readings {
            let better = if reading.higher_is_better { Better::Higher } else { Better::Lower };
            if numbers.last().is_none_or(|number| number.name != reading.number) {
                numbers.push(Number { name: reading.number.clone(), better, now: None, before: None });
            }
            let number = numbers.last_mut()?;
            let headline = Some((reading.figure, reading.observations));
            match weeks.iter().position(|week| *week == reading.week) {
                Some(0) => number.now = headline,
                Some(_) => number.before = headline,
                None => {}
            }
        }
        Some(Self { recorded_at, numbers })
    }

    pub fn worsened(&self) -> Vec<&str> {
        self.numbers.iter().filter(|number| number.worse() == Some(true)).map(|number| number.name.as_str()).collect()
    }

    pub fn score(&self) -> String {
        let compared = self.numbers.iter().filter(|number| number.worse().is_some()).count();
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
    scoreboard::tell(&north_graph_loops::graph_path(), &record.readings(), now)?;
    write(SCORE, &format!("{}\n", record.score()))?;
    Ok((record, measures))
}

fn stored() -> Result<Option<Record>, String> {
    let graph = north_graph_loops::graph_path();
    if !graph.exists() {
        return Ok(None);
    }
    Ok(Record::told(&scoreboard::read(&graph)?))
}

/// What the proxy's fleet tick says: the record is refreshed when a week old, and the numbers that got
/// worse are told once per recording.
pub fn tick(now: i64) -> Vec<String> {
    let current = match stored() {
        Ok(record) => record.filter(|record| (0..WEEK_S).contains(&(now - record.recorded_at))),
        Err(why) => return vec![format!("scoreboard not read: {why}")],
    };
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
    vec![format!("{} (recorded {}; north ledger scoreboard --weekly prints the figures)", record.score(), Timestamp::from_unix_seconds(record.recorded_at).local_twelve_hour())]
}

#[cfg(test)]
#[path = "weekly_tests.rs"]
mod tests;
