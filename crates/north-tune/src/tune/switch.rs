//! The skill switch's daily reading: the tick runs experiment L1's walk-forward backtest over the ledger and tells each
//! decision's reading to the graph, so a learned value that loses to its constant stops driving its decision until it
//! wins again, and prints one line per decision.
use crate::foundation::clock::Timestamp;
use crate::ledger::threads_database;
use crate::threads::calibration::backtest::{Reading, backtest_in};
use north_graph_loops::skill::{Skill, decision, tell_skills};

pub fn skill(reading: &Reading) -> Option<Skill> {
    Some(Skill { decision: decision(reading.decision)?, n: reading.n, learned: reading.learned, constant: reading.constant, value: reading.value })
}

/// A reading as one line of the L1 table.
pub fn line(reading: &Reading, skill: &Skill) -> String {
    let verdict = match (skill.judged(), skill.beats()) {
        (false, _) => format!("unjudged, n under {}, so the learned value keeps driving", skill.decision.min_n()),
        (true, true) => "beats its constant, so the learned value drives".to_owned(),
        (true, false) => "does not beat its constant, so the decision reads the constant".to_owned(),
    };
    let registered = reading.registered.as_ref().map_or_else(|| "no registered L1 line".to_owned(), |(passes, what)| format!("{what}, L1 line {}", if *passes { "passes" } else { "fails" }));
    format!("- {}: learned {:.3} against constant {:.3} ({}, n {}); {verdict}; {registered}", reading.decision, reading.learned, reading.constant, reading.score, reading.n)
}

/// Runs the backtest and, unless `dry_run`, tells the switch; returns the lines to print.
pub fn judge(dry_run: bool) -> Vec<String> {
    let now = Timestamp::now().unix_seconds();
    let readings = match backtest_in(&threads_database(), now) {
        Ok(readings) => readings,
        Err(error) => return vec![format!("skill switch: backtest unread, so every learned value keeps its last verdict: {error}")],
    };
    let skills: Vec<(Skill, &Reading)> = readings.iter().filter_map(|reading| Some((skill(reading)?, reading))).collect();
    let mut lines = vec!["skill switch (L1, walk-forward over the ledger's last 14 days, north#1221):".to_owned()];
    lines.extend(skills.iter().map(|(skill, reading)| line(reading, skill)));
    if !dry_run {
        let told: Vec<Skill> = skills.into_iter().map(|(skill, _)| skill).collect();
        if let Err(error) = tell_skills(&north_graph_loops::graph_path(), &told, now) {
            lines.push(format!("skill switch: untold, so every learned value keeps its last verdict: {error}"));
        }
    }
    lines
}
