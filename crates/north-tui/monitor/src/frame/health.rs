use ratatui::style::{Modifier, Stylize};
use ratatui::text::{Line, Span};

use super::paint::{Boxed, Fresh, Painter, dim, label, mark, style, title, trend, unknown_line};
use super::queue::failure_rate;
use crate::budget::Tone;
use crate::snapshot::{Builds, Health, Holds, Snapshot};
use crate::text::{clock, duration, sparkline_f};

const HEALTH_BOUND: u64 = 15;
const HEALTH_WRITTEN_BOUND: u64 = 1200;
const BUILDS_BOUND: u64 = 45;
const LADDER_BOUND: u64 = 8;
const HOLDS_BOUND: u64 = 90;

fn ladder_line(painter: &mut Painter, snapshot: &Snapshot) -> Line<'static> {
    let fresh = painter.fresh("ladder", &snapshot.ladder, LADDER_BOUND);
    let mut spans = vec![label("LADDER ")];
    match snapshot.ladder.value {
        Some(level) => {
            spans.push(painter.metric("ladder", f64::from(level), format!(" L{level} ")).add_modifier(Modifier::BOLD | Modifier::REVERSED));
            if level == 0 {
                spans.push(Span::styled("  full north", dim()));
            }
        }
        None => spans.push(Span::styled("UNKNOWN", style(Tone::Red))),
    }
    spans.extend(mark(&fresh));
    Line::from(spans)
}

fn checks_lines(painter: &mut Painter, health: &Health, fresh: &Fresh) -> Vec<Line<'static>> {
    let red = health.red.len();
    let mut spans = vec![
        label("holdlist "),
        Span::styled(format!("{} green", health.green), style(Tone::Green)),
        label(" · "),
        Span::styled(format!("{} amber", health.amber.len()), style(if health.amber.is_empty() { Tone::Plain } else { Tone::Amber })),
        label(" · "),
        painter.metric("health_red", red as f64, format!("{red} red")),
    ];
    let written = painter.now.saturating_sub(health.written);
    if written > HEALTH_WRITTEN_BOUND {
        painter.alert(Tone::Amber, format!("health.json STALE {}", duration(written)));
        spans.push(Span::styled(format!(" health.json STALE {}", duration(written)), style(Tone::Amber)));
    }
    spans.extend(mark(fresh));
    let mut lines = vec![Line::from(spans)];
    let named = |head: &str, names: &[String], tone: Tone| Line::from(vec![label(head.to_owned()), Span::styled(names.join(", "), style(tone))]);
    if !health.red.is_empty() {
        lines.push(named("  red: ", &health.red, Tone::Red));
    }
    if !health.amber.is_empty() {
        lines.push(named("  new: ", &health.amber, Tone::Amber));
    }
    lines
}

fn holds_lines(painter: &mut Painter, holds: &Holds) -> Vec<Line<'static>> {
    let red = holds.holds.iter().filter(|hold| hold.red.is_some()).count();
    let mut lines = vec![Line::from(vec![label("hand stops "), Span::raw(format!("{} standing · ", holds.holds.len())), Span::styled(format!("{red} red"), style(if red == 0 { Tone::Plain } else { Tone::Red }))])];
    for hold in &holds.holds {
        let tone = if hold.red.is_some() { Tone::Red } else { Tone::Plain };
        if let Some(why) = &hold.red {
            painter.alert(Tone::Red, format!("{} {}: {why}", hold.host, hold.kind));
        }
        lines.push(Line::from(Span::styled(format!("  {} {}: {} · by {}", hold.host, hold.kind, hold.why, hold.set_by), style(tone))));
        lines.push(Line::from(Span::styled(format!("    lifts when {}", hold.lifts_when), dim())));
    }
    lines.extend(holds.unread.iter().map(|(host, why)| Line::from(Span::styled(format!("  {host}: holds not read: {why}"), style(Tone::Amber)))));
    lines
}

fn seconds(painter: &mut Painter, id: &str, value: Option<f64>) -> Span<'static> {
    value.map_or_else(|| Span::styled("?", dim()), |seconds| painter.metric(id, seconds, duration(seconds.round() as u64)))
}

fn builds_lines(painter: &mut Painter, builds: &Builds, fresh: &Fresh, offset_s: i32) -> Vec<Line<'static>> {
    let mut spans = vec![
        label("compile "),
        seconds(painter, "compile_s", builds.compile_s),
        label("  test "),
        seconds(painter, "test_s", builds.test_s),
        label("  land checks "),
        seconds(painter, "land_checks_s", builds.land_checks_s),
    ];
    spans.extend(mark(fresh));
    let mut lines = vec![Line::from(spans)];
    if let Some(ended) = builds.ended_at {
        lines.push(Line::from(Span::styled(format!("  last land check ended {} ({} ago)", clock(ended, offset_s), painter.ago(ended)), dim())));
    }
    lines
}

fn rate_line(painter: &mut Painter, snapshot: &Snapshot) -> Line<'static> {
    let Some(landing) = &snapshot.landing.value else {
        return Line::from(vec![label("failure rate 6h "), Span::styled("UNKNOWN", style(Tone::Red))]);
    };
    let rate = failure_rate(landing);
    let budget = painter.budgets.get("failure_rate").map(|budget| format!(" (budget amber {:.0}%, red {:.0}%)", budget.amber, budget.red)).unwrap_or_default();
    let value = Span::styled(format!("{rate:.0}%"), style(painter.budgets.tone("failure_rate", rate)));
    Line::from(vec![label("failure rate 6h "), value, Span::styled(budget, dim())])
}

pub fn render(painter: &mut Painter, snapshot: &Snapshot, offset_s: i32) -> Boxed {
    let mut lines = vec![ladder_line(painter, snapshot)];
    let fresh = painter.fresh("health", &snapshot.health, HEALTH_BOUND);
    lines.extend(unknown_line("health", &fresh));
    if let Some(health) = &snapshot.health.value {
        lines.extend(checks_lines(painter, health, &fresh));
    }
    let holds_fresh = painter.fresh("holds", &snapshot.holds, HOLDS_BOUND);
    lines.extend(unknown_line("holds", &holds_fresh));
    if let Some(holds) = &snapshot.holds.value {
        lines.extend(holds_lines(painter, holds));
    }
    let builds_fresh = painter.fresh("builds", &snapshot.builds, BUILDS_BOUND);
    lines.extend(unknown_line("builds", &builds_fresh));
    if let Some(builds) = &snapshot.builds.value {
        lines.extend(builds_lines(painter, builds, &builds_fresh, offset_s));
    }
    lines.push(rate_line(painter, snapshot));
    lines.push(Line::from(vec![label("load 10 min "), Span::styled(sparkline_f(&snapshot.trends.load), trend())]));
    lines.push(Line::from(Span::styled(format!("budgets: {}", painter.budgets.source), dim())));
    let source = snapshot.health.value.as_ref().map_or("health.json", |health| health.source.as_str()).to_owned();
    (title("Health", &format!("{source} + spans"), &fresh), lines)
}
