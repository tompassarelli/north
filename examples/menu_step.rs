#[allow(dead_code)]
#[path = "../src/menu_bindings.rs"]
mod menu_bindings;
#[path = "../src/galileo_menu.rs"]
mod galileo_menu;

use galileo_core::{Entity, Value, history::History};
use galileo_runtime::{Program, Session};
use menu_bindings::{Event, Menu, North};
use std::hint::black_box;
use std::time::Instant;

fn populated(program: &Program) -> Result<Session<'_>, galileo_runtime::Error> {
    let north = North::named("north-main");
    let mut session = Session::new(program);
    session.run(Event::OpenSettingsMenu {
        north: north.clone(), kind: "config".into(), title: "Context Switchboard".into(),
    })?;
    session.run(Event::OfferSettingsRow {
        north: north.clone(), key: "row".into(), label: "Row".into(),
        description: "Settings entry".into(), annotation: "on".into(),
    })?;
    let prototype = session.view::<Menu>((north,))?.rows.remove(0).0;
    let mut run = galileo_core::state::Run::new(program);
    run.state = session.state().clone();
    let prototype_facts = run.state.facts.iter()
        .filter(|(_, values)| values.contains(&Value::Entity(prototype.clone())))
        .cloned().collect::<Vec<_>>();
    for fact in &prototype_facts { run.state.facts.remove(fact); }
    run.state.live.remove(&prototype);
    let order = program.field_relation(&prototype.entity_type, "order").unwrap();
    let key = program.field_relation(&prototype.entity_type, "key").unwrap();
    for index in 0..1000 {
        let row = Entity::created(prototype.entity_type.clone(), format!("{}#{index}", prototype.entity_type), index);
        run.state.live.insert(row.clone());
        for (relation, values) in &prototype_facts {
            let mut values = values.iter().map(|value| {
                if *value == Value::Entity(prototype.clone()) { Value::Entity(row.clone()) }
                else { value.clone() }
            }).collect::<Vec<_>>();
            if *relation == order { values[1] = Value::Int(index as i64); }
            if *relation == key { values[1] = Value::Text(format!("row-{index}")); }
            run.state.facts.insert((*relation, values));
        }
    }
    let count = program.relation_id("examples/north-menu/menu-row-count").unwrap();
    run.state.facts.retain(|(relation, _)| *relation != count);
    run.state.facts.insert((count, vec![North::named("north-main").into_value(), Value::Int(1000)]));
    run.state.next = 1000;
    run.history = History::new(&run.state);
    let mut checkpoint = run.checkpoint();
    checkpoint.program_identity = session.checkpoint().program_identity;
    Session::restore(program, checkpoint)
}

fn main() -> Result<(), String> {
    let program = galileo_menu::program().map_err(|error| format!("image: {error:?}"))?;
    let mut session = populated(&program).map_err(|error| format!("setup: {error:?}"))?;
    println!("setup=1000 rows from one real OfferSettingsRow checkpoint; default budget; timing=MoveMenu(delta=1)+Session::view<Menu>+five generated row field reads; unavailable title/help/empty/position excluded");
    let mut timings = Vec::with_capacity(100);
    for index in 0..100 {
        let started = Instant::now();
        let menu = galileo_menu::move_selection(&mut session, 1)
            .map_err(|error| format!("step {index}: elapsed_ns={} error={error:?}", started.elapsed().as_nanos()))?;
        for row in &menu.rows {
            let fields = (|| -> Result<_, galileo_runtime::EvalError> {
                Ok((row.key(session.state(), &program)?, row.label(session.state(), &program)?,
                    row.description(session.state(), &program)?, row.annotation(session.state(), &program)?,
                    row.order(session.state(), &program)?))
            })().map_err(|error| format!("row fields: {error:?}"))?;
            black_box(fields);
        }
        timings.push(started.elapsed().as_nanos());
        if menu.rows.len() != 1000 || menu.selection != index + 1 {
            return Err(format!("step {index}: rows={} selection={}", menu.rows.len(), menu.selection));
        }
    }
    timings.sort_unstable();
    println!("rows=1000 steps=100 median_ns={} p95_ns={} min_ns={} max_ns={} revision={}",
        timings[50], timings[94], timings[0], timings[99], session.revision());
    if timings[50] >= 1_000_000 { return Err("A8 median misses 1 ms".into()); }
    Ok(())
}
