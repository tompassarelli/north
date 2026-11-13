use std::fs::{self, OpenOptions};
use std::io::{self, Stdout, Write};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::{CrosstermBackend, TestBackend};

use crate::Options;
use crate::budget::Budgets;
use crate::collect::{self, HostTarget, Update, fleet::LOCAL};
use crate::frame::{View, draw};
use crate::snapshot::Snapshot;

struct Raw;

impl Raw {
    fn enter() -> io::Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        terminal.hide_cursor()?;
        Ok((Self, terminal))
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

fn offset_s() -> i32 {
    collect::run("date", &["+%z"], Duration::from_secs(2))
        .ok()
        .and_then(|text| {
            let text = text.trim();
            let sign = if text.starts_with('-') { -1 } else { 1 };
            let digits = text.trim_start_matches(['+', '-']);
            let hours: i32 = digits.get(0..2)?.parse().ok()?;
            let minutes: i32 = digits.get(2..4)?.parse().ok()?;
            Some(sign * (hours * 3600 + minutes * 60))
        })
        .unwrap_or(0)
}

fn quit_pressed(wait: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !event::poll(left)? {
            return Ok(false);
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return Ok(true);
        }
    }
}

fn absorb(snapshot: &mut Snapshot, updates: &Receiver<Update>) {
    let at = collect::now();
    for update in updates.try_iter() {
        collect::apply(snapshot, update, at);
    }
    snapshot.at = at;
    let busy = snapshot.fleet.value.as_ref().map(|fleet| fleet.busy());
    let load = snapshot.hosts.iter().find(|host| host.name == LOCAL).and_then(|host| host.state.value.as_ref()?.load1);
    snapshot.trends.sample(at, busy, load);
}

fn append(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let line = serde_json::to_string(snapshot).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    writeln!(file, "{line}").map_err(|error| format!("{}: {error}", path.display()))
}

pub fn live(options: &Options, budgets: &Budgets, hosts: Vec<HostTarget>) -> Result<(), String> {
    let mut snapshot = collect::seed(&hosts);
    let updates = collect::spawn(hosts);
    let offset_s = offset_s();
    let (_raw, mut terminal) = Raw::enter().map_err(|error| error.to_string())?;
    loop {
        absorb(&mut snapshot, &updates);
        if let Some(path) = &options.record {
            append(path, &snapshot)?;
        }
        let view = View { snapshot: &snapshot, budgets, offset_s, mode: "live" };
        terminal.draw(|frame| draw(frame, &view)).map_err(|error| error.to_string())?;
        if quit_pressed(Duration::from_millis(options.interval_ms)).map_err(|error| error.to_string())? {
            return Ok(());
        }
    }
}

fn print_frame(options: &Options, view: &View) -> Result<(), String> {
    let Ok(mut terminal) = Terminal::new(TestBackend::new(options.size.0, options.size.1));
    let Ok(_) = terminal.draw(|frame| draw(frame, view));
    print!("{}", crate::ansi::render(terminal.backend().buffer(), options.ansi));
    Ok(())
}

fn settled(snapshot: &Snapshot) -> bool {
    let readings = [snapshot.fleet.at, snapshot.landing.at, snapshot.landers.at, snapshot.lag.at, snapshot.rollout.at, snapshot.health.at, snapshot.builds.at, snapshot.ladder.at];
    readings.iter().all(Option::is_some) && snapshot.hosts.iter().all(|host| host.state.at.is_some())
}

pub fn once(options: &Options, budgets: &Budgets, hosts: Vec<HostTarget>) -> Result<(), String> {
    let mut snapshot = collect::seed(&hosts);
    let updates = collect::spawn(hosts);
    let deadline = Instant::now() + Duration::from_secs(options.settle_s);
    while Instant::now() < deadline && !settled(&snapshot) {
        std::thread::sleep(Duration::from_millis(200));
        absorb(&mut snapshot, &updates);
    }
    absorb(&mut snapshot, &updates);
    if let Some(path) = &options.record {
        append(path, &snapshot)?;
    }
    print_frame(options, &View { snapshot: &snapshot, budgets, offset_s: offset_s(), mode: "live" })
}

fn load(path: &Path) -> Result<Vec<Snapshot>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    text.lines().filter(|line| !line.trim().is_empty()).enumerate().map(|(index, line)| serde_json::from_str(line).map_err(|error| format!("{} line {}: {error}", path.display(), index + 1))).collect()
}

pub fn replay(options: &Options, budgets: &Budgets, path: &Path) -> Result<(), String> {
    let snapshots = load(path)?;
    let offset_s = offset_s();
    if options.once {
        let index = options.frame.unwrap_or(snapshots.len().saturating_sub(1));
        let snapshot = snapshots.get(index).ok_or_else(|| format!("{} holds {} snapshots", path.display(), snapshots.len()))?;
        let mode = format!("replay {}/{}", index + 1, snapshots.len());
        return print_frame(options, &View { snapshot, budgets, offset_s, mode: &mode });
    }
    let (_raw, mut terminal) = Raw::enter().map_err(|error| error.to_string())?;
    let mut index = 0;
    loop {
        let snapshot = &snapshots[index.min(snapshots.len().saturating_sub(1))];
        let mode = format!("replay x{} {}/{}", options.speed, index + 1, snapshots.len());
        terminal.draw(|frame| draw(frame, &View { snapshot, budgets, offset_s, mode: &mode })).map_err(|error| error.to_string())?;
        let gap = snapshots.get(index + 1).map_or(1.5, |next| (next.at.saturating_sub(snapshot.at) as f64 / options.speed).clamp(0.2, 2.0));
        if quit_pressed(Duration::from_secs_f64(gap)).map_err(|error| error.to_string())? {
            return Ok(());
        }
        if index + 1 < snapshots.len() {
            index += 1;
        }
    }
}

pub fn rebuild(options: &Options, (from, to): (u64, u64), hosts: &[HostTarget]) -> Result<(), String> {
    let path = options.record.as_ref().ok_or("--rebuild needs --record FILE")?;
    let snapshots = crate::rebuild::snapshots(&collect::state_dir(), from, to, options.step, hosts)?;
    for snapshot in &snapshots {
        append(path, snapshot)?;
    }
    eprintln!("north-monitor: wrote {} snapshots to {}", snapshots.len(), path.display());
    Ok(())
}
