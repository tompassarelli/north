use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use super::paint::{Boxed, Painter, dim, label, mark, title, trend, unknown_line};
use crate::snapshot::{Fleet, Snapshot};
use crate::text::sparkline;

const FLEET_BOUND: u64 = 10;
const STATES: [&str; 5] = ["working", "starting", "idle", "parked", "queued"];

fn headline(painter: &mut Painter, fleet: &Fleet) -> Line<'static> {
    let bold = |span: Span<'static>| span.add_modifier(Modifier::BOLD);
    let (busy, idle, queued) = (fleet.busy(), fleet.idle(), fleet.count("queued"));
    Line::from(vec![
        label("BUSY "),
        bold(painter.metric("fleet_busy", f64::from(busy), busy.to_string())),
        label("   IDLE "),
        bold(painter.metric("fleet_idle", f64::from(idle), idle.to_string())),
        label("   QUEUED "),
        bold(painter.metric("fleet_queued", f64::from(queued), queued.to_string())),
    ])
}

fn breakdown(fleet: &Fleet) -> Line<'static> {
    Line::from(STATES.iter().flat_map(|state| [label(format!("{state} ")), Span::raw(format!("{}  ", fleet.count(state)))]).collect::<Vec<_>>())
}

fn servers(fleet: &Fleet) -> Vec<Line<'static>> {
    let header = Line::from(Span::styled(format!("{:<12} {:>5} {:>5} {:>5} {:>5} {:>5}", "server", "work", "start", "idle", "park", "queue"), dim()));
    let rows = fleet.servers.iter().map(|(server, counts)| {
        let cells: Vec<String> = STATES.iter().map(|state| format!("{:>5}", counts.get(*state).copied().unwrap_or(0))).collect();
        Line::from(format!("{server:<12} {}", cells.join(" ")))
    });
    std::iter::once(header).chain(rows).collect()
}

pub fn render(painter: &mut Painter, snapshot: &Snapshot) -> Boxed {
    let fresh = painter.fresh("fleet", &snapshot.fleet, FLEET_BOUND);
    let source = snapshot.fleet.value.as_ref().map_or("fleet", |fleet| fleet.source.as_str()).to_owned();
    let mut lines: Vec<Line<'static>> = unknown_line("fleet", &fresh).into_iter().collect();
    if let Some(fleet) = &snapshot.fleet.value {
        lines.push(headline(painter, fleet));
        lines.push(breakdown(fleet));
        lines.push(Line::from(vec![label("busy, 10 min "), Span::styled(sparkline(&snapshot.trends.busy), trend())]));
        lines.extend(servers(fleet));
        let mut tail = vec![label("busy : idle "), Span::styled(format!("{} : {}", fleet.busy(), fleet.idle()), Style::new().add_modifier(Modifier::BOLD))];
        tail.extend(mark(&fresh));
        lines.push(Line::from(tail));
    }
    (title("Fleet", &source, &fresh), lines)
}
