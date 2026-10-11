use crate::menu_bindings::{Event, Menu, North};
use galileo_runtime::{Error, Program, Session};

pub fn program() -> Result<Program, Error> {
    Program::load(include_bytes!("menu_program.galileo"))
}

pub fn move_selection(session: &mut Session<'_>, delta: i64) -> Result<Menu, Error> {
    let north = North::named("north-main");
    session.run(Event::MoveMenu { north: north.clone(), delta })?;
    Ok(session.view::<Menu>((north,))?)
}
