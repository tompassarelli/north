#[allow(dead_code)]
#[path = "../src/menu_bindings.rs"]
mod menu_bindings;
#[allow(dead_code)]
#[path = "../src/galileo_menu.rs"]
mod galileo_menu;

use std::hint::black_box;
use std::time::Instant;

fn main() -> Result<(), String> {
    let program = galileo_menu::program().map_err(|error| format!("image: {error:?}"))?;
    let mut session = galileo_menu::MenuSession::new(&program).map_err(|error| format!("new: {error:?}"))?;
    let entries = (0..1000).map(|index| (format!("row-{index}"), format!("Row {index}"))).collect::<Vec<_>>();
    let rows = entries.iter().map(|(key, label)| (key.as_str(), label.as_str(), "Settings entry", "on")).collect::<Vec<_>>();
    let setup_started = Instant::now();
    session.open_settings_menu("config", "Context Switchboard", &rows).map_err(|error| format!(
        "setup one run_all of open_settings_menu sequence with 1000 rows: elapsed_ns={} error={error:?}",
        setup_started.elapsed().as_nanos(),
    ))?;
    println!("setup=one real run_all with clear/open/1000 offer/query events; default budget; timing=MoveMenu(delta=1)+Session::view<Menu>");
    let mut timings = Vec::with_capacity(100);
    for index in 0..100 {
        let started = Instant::now();
        session.move_menu(1).map_err(|error| format!("step {index}: elapsed_ns={} error={error:?}", started.elapsed().as_nanos()))?;
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
