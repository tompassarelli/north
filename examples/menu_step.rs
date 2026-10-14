#[allow(dead_code)]
#[path = "../src/menu.rs"]
mod menu;

use std::hint::black_box;
use std::time::Instant;

fn main() -> Result<(), String> {
    let mut session = menu::MenuSession::default();
    let entries = (0..1000).map(|index| (format!("row-{index}"), format!("Row {index}"))).collect::<Vec<_>>();
    let rows = entries.iter().map(|(key, label)| (key.as_str(), label.as_str(), "Settings entry", "on")).collect::<Vec<_>>();
    let setup_started = Instant::now();
    session.open_settings_menu("config", "Context Switchboard", &rows).map_err(|error| error.to_string())?;
    println!("setup_ns={} rows=1000; timing=Move(1)+menu projection", setup_started.elapsed().as_nanos());
    let mut timings = Vec::with_capacity(100);
    for index in 0..100 {
        let started = Instant::now();
        session.move_menu(1).map_err(|error| error.to_string())?;
        black_box(session.menu());
        timings.push(started.elapsed().as_nanos());
        if session.menu().rows.len() != 1000 || session.menu().selection != index + 1 {
            return Err(format!("step {index}: rows={} selection={}", session.menu().rows.len(), session.menu().selection));
        }
    }
    timings.sort_unstable();
    println!("rows=1000 steps=100 median_ns={} p95_ns={} min_ns={} max_ns={} revision={}",
        timings[50], timings[94], timings[0], timings[99], session.revision());
    if timings[50] >= 1_000_000 { return Err("A8 median misses 1 ms".into()); }
    Ok(())
}
