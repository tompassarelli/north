use std::collections::BTreeMap;

use ratatui::style::{Modifier, Stylize};
use ratatui::text::{Line, Span};

use super::paint::{Boxed, Fresh, Painter, dim, label, mark, style, title, tone_of, trend, unknown_line};
use crate::budget::Tone;
use crate::snapshot::{Decided, Landing, Snapshot};
use crate::text::{duration, sparkline};

const LANDING_BOUND: u64 = 12;

pub fn failure_rate(landing: &Landing) -> f64 {
    if landing.decided_6h == 0 { 0.0 } else { f64::from(landing.failed_6h) * 100.0 / f64::from(landing.decided_6h) }
}

fn direction(painter: &mut Painter, landing: &Landing) -> Span<'static> {
    let growth = i64::from(landing.depth) - i64::from(landing.depth_5m_ago);
    match (growth, landing.depth) {
        (0, 0) => Span::styled("EMPTY", style(Tone::Green)),
        (0, _) => Span::styled("NOT DRAINING", style(Tone::Amber)),
        (shrink, _) if shrink < 0 => Span::styled("DRAINING", style(Tone::Green).add_modifier(Modifier::BOLD)),
        _ => painter.metric("queue_growth", growth as f64, "GROWING"),
    }
}

fn depth_line(painter: &mut Painter, landing: &Landing, fresh: &Fresh) -> Line<'static> {
    let depth = painter.metric("queue_depth", f64::from(landing.depth), landing.depth.to_string()).add_modifier(Modifier::BOLD);
    let mut spans = vec![label("DEPTH "), depth, label(format!("   5 min ago {}  ", landing.depth_5m_ago)), direction(painter, landing)];
    spans.extend(mark(fresh));
    Line::from(spans)
}

fn time_line(painter: &mut Painter, landing: &Landing) -> Line<'static> {
    let mut timed = |id: &str, value: Option<u64>| value.map_or_else(|| Span::styled("none", dim()), |seconds| painter.metric(id, seconds as f64, duration(seconds)));
    let p50 = timed("land_p50", landing.land_p50_s);
    let p95 = timed("land_p95", landing.land_p95_s);
    Line::from(vec![label("time to land 1h  p50 "), p50, label("  p95 "), p95])
}

fn landing_lines(painter: &mut Painter, landing: &Landing, fresh: &Fresh) -> Vec<Line<'static>> {
    let rate = failure_rate(landing);
    vec![
        depth_line(painter, landing, fresh),
        Line::from(vec![label("depth 1h      "), Span::styled(sparkline(&landing.depth_trend), trend())]),
        Line::from(vec![label("landed/h 24h  "), Span::styled(sparkline(&landing.landed_per_hour), trend()), label(format!("  {} in the last hour", landing.landed_last_hour))]),
        time_line(painter, landing),
        Line::from(vec![label("failed 6h "), painter.metric("failure_rate", rate, format!("{rate:.0}%")), label(format!(" ({} of {} decided)", landing.failed_6h, landing.decided_6h))]),
    ]
}

fn landers_line(painter: &mut Painter, snapshot: &Snapshot) -> Line<'static> {
    let fresh = painter.fresh("landers", &snapshot.landers, LANDING_BOUND);
    if let Some(line) = unknown_line("landers", &fresh) {
        return line;
    }
    let landers = snapshot.landers.value.as_deref().unwrap_or_default();
    let down: Vec<&str> = landers.iter().filter(|lander| !lander.up).map(|lander| lander.repo.as_str()).collect();
    let mut builds: BTreeMap<&str, u32> = BTreeMap::new();
    for lander in landers.iter().filter(|lander| lander.up) {
        *builds.entry(lander.build.as_deref().unwrap_or("?")).or_default() += 1;
    }
    let runs: Vec<String> = builds.iter().map(|(build, count)| format!("{build} x{count}")).collect();
    let mut spans = vec![label("landers "), Span::styled(format!("{} up", landers.len() - down.len()), style(Tone::Green)), Span::styled(format!(" on {}", runs.join(", ")), dim())];
    if !down.is_empty() {
        spans.push(label("  down "));
        spans.push(painter.metric("landers_down", down.len() as f64, down.join(" ")));
    }
    spans.extend(mark(&fresh));
    Line::from(spans)
}

fn decided_line(painter: &Painter, decided: &Decided) -> Line<'static> {
    let word = decided.verdict.split_whitespace().next().unwrap_or("undecided").to_owned();
    let branch: String = decided.branch.chars().take(22).collect();
    Line::from(vec![
        Span::raw(format!("{:>10} ", format!("{} ago", painter.ago(decided.at)))),
        Span::raw(format!("{:<9.9} {branch:<22} ", decided.repo)),
        Span::styled(format!("{word:<9}"), style(tone_of(&word))),
        Span::styled(duration(decided.seconds), dim()),
    ])
}

pub fn render(painter: &mut Painter, snapshot: &Snapshot) -> Boxed {
    let fresh = painter.fresh("landing queue", &snapshot.landing, LANDING_BOUND);
    let mut lines: Vec<Line<'static>> = unknown_line("landing queue", &fresh).into_iter().collect();
    if let Some(landing) = &snapshot.landing.value {
        lines.extend(landing_lines(painter, landing, &fresh));
    }
    lines.push(landers_line(painter, snapshot));
    if let Some(landing) = &snapshot.landing.value {
        lines.push(Line::from(Span::styled("last decided", dim())));
        lines.extend(landing.recent.iter().map(|decided| decided_line(painter, decided)));
    }
    (title("Queue & Lander", "land-queue", &fresh), lines)
}
