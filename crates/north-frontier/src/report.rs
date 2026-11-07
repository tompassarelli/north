use std::collections::BTreeMap;
use std::fmt::Write;

use crate::rows::{Arm, Run};
use crate::stats::{Interval, Pairs, bootstrap, mcnemar, mean, median, newcombe_paired, wilson};

/// A task both arms ran, with neither run's transcript touching the landed commit.
struct Pair<'a> {
    north: &'a Run,
    plain: &'a Run,
}

fn pair_up(runs: &[Run]) -> (Vec<Pair<'_>>, usize) {
    let mut by_task: BTreeMap<&str, (Option<&Run>, Option<&Run>)> = BTreeMap::new();
    for run in runs {
        let slot = by_task.entry(&run.task).or_default();
        match run.arm {
            Arm::North => slot.0 = Some(run),
            Arm::Plain => slot.1 = Some(run),
        }
    }
    let mut left_out = 0;
    let mut pairs = Vec::new();
    for (north, plain) in by_task.into_values() {
        match (north, plain) {
            (Some(north), Some(plain)) if !north.contaminated && !plain.contaminated => pairs.push(Pair { north, plain }),
            _ => left_out += 1,
        }
    }
    (pairs, left_out)
}

fn arm(pair: &Pair<'_>, which: Arm) -> Run {
    match which {
        Arm::North => pair.north.clone(),
        Arm::Plain => pair.plain.clone(),
    }
}

fn cost_per_correct(pairs: &[&Pair<'_>], which: Arm) -> Option<f64> {
    let runs: Vec<Run> = pairs.iter().map(|pair| arm(pair, which)).collect();
    let correct = runs.iter().filter(|run| run.correct).count();
    (correct > 0).then(|| runs.iter().map(|run| run.cost_usd).sum::<f64>() / correct as f64)
}

fn paired_cost_gap(pairs: &[&Pair<'_>]) -> Option<f64> {
    Some(cost_per_correct(pairs, Arm::North)? - cost_per_correct(pairs, Arm::Plain)?)
}

fn correct_walls(pairs: &[&Pair<'_>], which: Arm) -> Vec<f64> {
    pairs.iter().map(|pair| arm(pair, which)).filter(|run| run.correct).map(|run| run.wall_seconds).collect()
}

fn wall_gap(pairs: &[&Pair<'_>]) -> Option<f64> {
    let gaps: Vec<f64> = pairs.iter().filter(|pair| pair.north.correct && pair.plain.correct).map(|pair| pair.north.wall_seconds - pair.plain.wall_seconds).collect();
    median(&gaps)
}

fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

fn span(interval: Option<Interval>, show: impl Fn(f64) -> String) -> String {
    interval.map_or_else(|| "no interval".into(), |found| format!("95% {} to {}", show(found.low), show(found.high)))
}

fn minutes(seconds: f64) -> String {
    format!("{:.1} min", seconds / 60.0)
}

fn dollars(amount: f64) -> String {
    format!("${amount:.2}")
}

fn rate_line(label: &str, pairs: &[Pair<'_>], pick: impl Fn(&Run) -> bool, which: Arm) -> String {
    let hits = pairs.iter().filter(|pair| pick(&arm(pair, which))).count();
    let share = if pairs.is_empty() { "n/a".into() } else { percent(hits as f64 / pairs.len() as f64) };
    format!("- {label}, {which:?}: {share} ({hits}/{}), {}\n", pairs.len(), span(wilson(hits, pairs.len()), percent))
}

fn pairs_of(pairs: &[Pair<'_>], pick: impl Fn(&Run) -> bool) -> Pairs {
    pairs.iter().fold(Pairs::default(), |mut counts, pair| {
        match (pick(pair.north), pick(pair.plain)) {
            (true, true) => counts.both += 1,
            (true, false) => counts.only_first += 1,
            (false, true) => counts.only_second += 1,
            (false, false) => counts.neither += 1,
        }
        counts
    })
}

fn signed_points(delta: f64) -> String {
    format!("{:+.0} points", delta * 100.0)
}

pub fn render(runs: &[Run]) -> String {
    let (pairs, left_out) = pair_up(runs);
    let mut out = String::new();
    let _ = writeln!(out, "frontier: north against plain Claude Code on {} boxes both arms ran ({left_out} left out: one arm missing or a transcript touched the landed commit)", pairs.len());
    if pairs.is_empty() {
        return out;
    }
    for (label, pick) in [("correct on first pass", (|run: &Run| run.correct_first_pass) as fn(&Run) -> bool), ("correct after continuations", |run: &Run| run.correct)] {
        for which in [Arm::North, Arm::Plain] {
            out.push_str(&rate_line(label, &pairs, pick, which));
        }
        let counts = pairs_of(&pairs, pick);
        if let Some((delta, interval)) = newcombe_paired(counts) {
            let _ = writeln!(out, "  north minus plain: {}, {}, exact McNemar p={:.3} ({} only north, {} only plain)", signed_points(delta), span(Some(interval), signed_points), mcnemar(counts), counts.only_first, counts.only_second);
        }
    }
    let refs: Vec<&Pair<'_>> = pairs.iter().collect();
    for which in [Arm::North, Arm::Plain] {
        let cost = cost_per_correct(&refs, which).map_or_else(|| "no correct run".into(), dollars);
        let _ = writeln!(out, "- dollars per correct task, {which:?}: {cost}");
    }
    if let Some(gap) = paired_cost_gap(&refs) {
        let _ = writeln!(out, "  north minus plain: {:+.2} USD, {}", gap, span(bootstrap(&pairs, paired_cost_gap), |value| format!("{value:+.2} USD")));
    }
    for which in [Arm::North, Arm::Plain] {
        let walls = correct_walls(&refs, which);
        let median_wall = median(&walls).map_or_else(|| "no correct run".into(), minutes);
        let spread = bootstrap(&walls, |sample| median(&sample.iter().map(|wall| **wall).collect::<Vec<_>>()));
        let _ = writeln!(out, "- wall time to a correct result, {which:?}: median {median_wall} over {} correct runs, {}", walls.len(), span(spread, minutes));
    }
    if let Some(gap) = wall_gap(&refs) {
        let _ = writeln!(out, "  north minus plain over tasks both passed: median {}, {}", minutes(gap), span(bootstrap(&pairs, wall_gap), minutes));
    }
    for which in [Arm::North, Arm::Plain] {
        let counts: Vec<f64> = pairs.iter().map(|pair| f64::from(arm(pair, which).interventions)).collect();
        let spread = bootstrap(&counts, |sample| mean(&sample.iter().map(|count| **count).collect::<Vec<_>>()));
        let _ = writeln!(out, "- interventions per task, {which:?}: {:.2}, {}", mean(&counts).unwrap_or_default(), span(spread, |value| format!("{value:.2}")));
    }
    out
}
