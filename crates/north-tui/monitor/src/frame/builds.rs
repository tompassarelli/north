use std::collections::BTreeMap;

use ratatui::style::{Modifier, Stylize};
use ratatui::text::{Line, Span};

use super::paint::{Boxed, Fresh, Painter, dim, label, mark, style, title, tone_of, unknown_line};
use crate::budget::Tone;
use crate::collect::fleet::LOCAL;
use crate::snapshot::{Host, Rollout, Snapshot};

const LOCAL_BOUND: u64 = 15;
const REMOTE_BOUND: u64 = 90;
const LAG_BOUND: u64 = 120;
const ROLLOUT_BOUND: u64 = 15;

fn runtime(host: &Host) -> Option<&str> {
    host.state.value.as_ref()?.runtime.as_deref()
}

fn common(hosts: &[Host]) -> Option<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for version in hosts.iter().filter_map(runtime) {
        *counts.entry(version).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, count)| *count).map(|(version, _)| version.to_owned())
}

fn headline(painter: &mut Painter, hosts: &[Host], common: Option<&str>) -> Line<'static> {
    let Some(common) = common else {
        painter.alert(Tone::Red, "no host runtime known".into());
        return Line::from(Span::styled("NO HOST RUNTIME KNOWN", style(Tone::Red).add_modifier(Modifier::REVERSED)));
    };
    let odd: Vec<String> = hosts.iter().filter_map(|host| runtime(host).filter(|version| *version != common).map(|version| format!("{} on {version}", host.name))).collect();
    let known = hosts.iter().filter(|host| runtime(host).is_some()).count();
    if odd.is_empty() {
        let text = format!("ALL {known} HOSTS ON {common}");
        return Line::from(vec![painter.metric("runtime_skew", 0.0, text).add_modifier(Modifier::BOLD)]);
    }
    let text = format!("DIFFERS: {}", odd.join(", "));
    Line::from(vec![painter.metric("runtime_skew", odd.len() as f64, text).add_modifier(Modifier::BOLD | Modifier::REVERSED), Span::styled(format!(" (rest on {common})"), dim())])
}

fn host_fresh(painter: &mut Painter, host: &Host) -> Fresh {
    let bound = if host.name == LOCAL { LOCAL_BOUND } else { REMOTE_BOUND };
    painter.fresh(&format!("host {}", host.name), &host.state, bound)
}

fn host_line(painter: &mut Painter, host: &Host, common: Option<&str>, old: u32) -> (Line<'static>, Fresh) {
    let fresh = host_fresh(painter, host);
    let mut spans = vec![Span::raw(format!("{:<12} ", host.name))];
    let Some(state) = &host.state.value else {
        spans.extend(mark(&fresh));
        return (Line::from(spans), fresh);
    };
    let version = state.runtime.clone().unwrap_or_else(|| "?".into());
    spans.push(match common {
        Some(common) if common != version => Span::styled(format!("{version:<7}!="), style(Tone::Red).add_modifier(Modifier::REVERSED)),
        _ => Span::styled(format!("{version:<9}"), style(Tone::Green)),
    });
    let follow = state.follow.clone().unwrap_or_else(|| "unknown".into());
    spans.push(Span::styled(format!(" {follow:<12}"), style(if follow == "active" { Tone::Green } else { Tone::Amber })));
    spans.push(state.load_per_core().map_or_else(|| Span::styled(format!("{:>9}", "?"), dim()), |load| painter.metric_of(&format!("{} ", host.name), "load_per_core", load, format!("{load:>9.2}"))));
    spans.push(Span::styled(format!("{old:>6}"), if old > 0 { style(Tone::Amber) } else { dim() }));
    spans.extend(mark(&fresh));
    (Line::from(spans), fresh)
}

fn channel_line(hosts: &[Host]) -> Line<'static> {
    let pins: Vec<String> = hosts.iter().filter_map(|host| Some(format!("{} {}", host.name, host.state.value.as_ref()?.stable.as_deref()?))).collect();
    let text = if pins.is_empty() { "channels: stable pin unknown".to_owned() } else { format!("channels: stable {}", pins.join(", ")) };
    Line::from(Span::styled(text, dim()))
}

fn old_processes(snapshot: &Snapshot) -> BTreeMap<String, u32> {
    let lag = snapshot.lag.value.clone().unwrap_or_default();
    let current: BTreeMap<&str, &str> = snapshot.hosts.iter().filter_map(|host| Some((host.name.as_str(), runtime(host)?))).collect();
    lag.into_iter()
        .map(|(host, runtimes)| {
            let now = current.get(host.as_str()).copied();
            let old = runtimes.iter().filter(|(version, _)| Some(version.as_str()) != now).map(|(_, count)| *count).sum();
            (host, old)
        })
        .collect()
}

fn lag_line(painter: &mut Painter, snapshot: &Snapshot, old: &BTreeMap<String, u32>) -> Line<'static> {
    let fresh = painter.fresh("runtime lag", &snapshot.lag, LAG_BOUND);
    let total: u32 = old.values().sum();
    let mut spans = vec![label("old-runtime processes ")];
    if let Some(line) = unknown_line("runtime lag", &fresh) {
        return line;
    }
    spans.push(painter.metric("stale_processes", f64::from(total), if total == 0 { "none".into() } else { total.to_string() }));
    spans.extend(mark(&fresh));
    Line::from(spans)
}

fn rollout_lines(painter: &mut Painter, rollout: &Rollout) -> Vec<Line<'static>> {
    let state = rollout.promote_state.clone().unwrap_or_else(|| "none".into());
    let word = state.split_whitespace().next().unwrap_or("none").to_owned();
    let since = rollout.promote_at.map(|at| format!(" {} ago", painter.ago(at))).unwrap_or_default();
    let promote = Line::from(vec![
        label("canary/promote "),
        Span::styled(state, style(tone_of(&word))),
        Span::raw(format!(" {}", rollout.promote_sha.clone().unwrap_or_default())),
        Span::styled(since, dim()),
    ]);
    let mut rollback = vec![label("rollbacks 24h "), painter.metric("rollbacks_24h", f64::from(rollout.rollbacks_24h), rollout.rollbacks_24h.to_string())];
    if let Some((host, at)) = &rollout.last_rollback {
        rollback.push(Span::styled(format!("  last {host} {} ago", painter.ago(*at)), dim()));
    }
    vec![promote, Line::from(rollback)]
}

pub fn render(painter: &mut Painter, snapshot: &Snapshot) -> Boxed {
    let common = common(&snapshot.hosts);
    let old = old_processes(snapshot);
    let mut lines = vec![headline(painter, &snapshot.hosts, common.as_deref())];
    lines.push(Line::from(Span::styled(format!("{:<12} {:<9} {:<12} {:>9} {:>6}", "host", "runtime", "follow timer", "load/core", "old"), dim())));
    let mut worst = Fresh::Ok(0);
    for host in &snapshot.hosts {
        let host_old = old.iter().find(|(name, _)| name.rsplit('@').next() == Some(host.name.as_str())).map_or(0, |(_, count)| *count);
        let (line, fresh) = host_line(painter, host, common.as_deref(), host_old);
        lines.push(line);
        if !fresh.is_ok() && worst.is_ok() {
            worst = fresh;
        }
    }
    lines.push(channel_line(&snapshot.hosts));
    lines.push(lag_line(painter, snapshot, &old));
    let rollout_fresh = painter.fresh("rollout", &snapshot.rollout, ROLLOUT_BOUND);
    if let Some(rollout) = &snapshot.rollout.value {
        lines.extend(rollout_lines(painter, rollout));
    }
    if let Some(line) = lines.last_mut() {
        line.spans.extend(mark(&rollout_fresh));
    }
    (title("Builds & Rollouts", "ssh + runtime lag", &worst), lines)
}
