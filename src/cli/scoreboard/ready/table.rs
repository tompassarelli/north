use std::fmt::Write;

use north_goals::registry::{Bound, Goal};

use super::{Bar, Goals, NEEDED_DAYS, Reading, Repo, View, counts};
use crate::cli::scoreboard::duration;
use crate::foundation::clock::Timestamp;

fn show(unit: &str, value: f64) -> String {
    match unit {
        "s" => duration(value),
        "%" => format!("{value:.0}%"),
        "per h" => format!("{value:.2}/h"),
        "min" => format!("{value:.0} min"),
        _ => format!("{value:.0}"),
    }
}

fn threshold(goal: &Goal) -> String {
    format!("{} {}", if matches!(goal.bound, Bound::Max(_)) { "<=" } else { ">=" }, show(&goal.unit, goal.bound.limit()))
}

const MARK: usize = 2;

fn cell(goal: &Goal, reading: Option<Reading>, explain: bool) -> String {
    match reading {
        Some(reading) if explain => format!("{} (n={})", show(&goal.unit, reading.value), reading.sample),
        Some(reading) => show(&goal.unit, reading.value),
        None => "none".into(),
    }
}

fn mark(text: &str, color: bool) -> String {
    match (color, text.trim_end()) {
        (true, "PASS") => format!("\x1b[32m{text}\x1b[0m"),
        (true, "FAIL") => format!("\x1b[1;31m{text}\x1b[0m"),
        _ => text.to_owned(),
    }
}

/// The first line of a repository's table: how many bars fail on the last 3 h, then whether the harness is ready, which needs every bar passing on 24 h for NEEDED_DAYS days in a row.
pub(super) fn headline(repo: &Repo) -> String {
    let failing = repo.bars.iter().zip(Bar::ALL).filter(|(cells, bar)| !counts(cells.three_hours.1, *bar)).count();
    let ready = repo.days_in_a_row >= NEEDED_DAYS;
    format!("{}: {failing} of {} bars fail on the last 3 h; {}: {} of {NEEDED_DAYS} days in a row with every bar passing on 24 h", repo.name, Bar::ALL.len(), if ready { "ready" } else { "not ready" }, repo.days_in_a_row)
}

fn table(out: &mut String, repo: &Repo, goals: &Goals, view: View) {
    let mut rows: Vec<Vec<String>> = vec![["bar", "target", "verdict (3 h)", "1 h", "3 h", "6 h", "24 h"].map(String::from).to_vec()];
    for (bar, cells) in Bar::ALL.iter().zip(&repo.bars) {
        let goal = goals.of(*bar);
        let passed = counts(cells.three_hours.1, *bar);
        rows.push(vec![bar.label().into(), threshold(goal), if passed { "PASS" } else { "FAIL" }.into(), cell(goal, cells.hour, true), cell(goal, cells.three_hours.0, view.explain), cell(goal, cells.six_hours, view.explain), cell(goal, cells.day, view.explain)]);
    }
    let _ = writeln!(out, "{}", headline(repo));
    let widths: Vec<usize> = (0..rows[0].len()).map(|column| rows.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    for row in &rows {
        let line: Vec<String> = row.iter().zip(&widths).enumerate().map(|(column, (text, width))| {
            let padded = format!("{text:<width$}");
            if column == MARK { mark(&padded, view.color) } else { padded }
        }).collect();
        let _ = writeln!(out, "{}", line.join("  ").trim_end());
    }
}

pub(super) fn render(repos: &[Repo], goals: &Goals, now: i64, view: View) -> String {
    let mut out = String::new();
    for (index, repo) in repos.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        table(&mut out, repo, goals, view);
    }
    if view.explain {
        let _ = writeln!(out, "\nat {}: each bar passes or fails on its last 3 h; 1 h, 6 h and 24 h are context with no verdict", Timestamp::from_unix_seconds(now).local_twelve_hour());
        let _ = writeln!(out, "days: the newest 24 h windows in a row in which every bar passed; n: each cell's sample (the 1 h cell always shows it)");
        let _ = writeln!(out, "targets: goals.toml, ready class");
        let _ = writeln!(out, "sources: landings ledger, land-queue spans, worker ledger, Tom's transcripts, GitHub mirror, pace-control history");
    }
    out
}
