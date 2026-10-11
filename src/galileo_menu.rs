use crate::menu_bindings::{Event, Menu, North};
use galileo_runtime::{Error, Program, Session};

pub fn program() -> Result<Program, Error> {
    Program::load(include_bytes!("menu_program.galileo"))
}

fn north() -> North { North::named("north-main") }

fn offer(key: &str, label: &str, description: &str, annotation: &str) -> Event {
    Event::OfferRow { north: north(), key: key.into(), label: label.into(), description: description.into(), annotation: annotation.into() }
}

pub struct MenuSession<'p> {
    session: Session<'p>,
    menu: Menu,
}

impl<'p> MenuSession<'p> {
    pub fn new(program: &'p Program) -> Result<Self, Error> {
        let session = Session::new(program);
        let menu = session.view::<Menu>((north(),))?;
        Ok(Self { session, menu })
    }

    pub fn menu(&self) -> &Menu { &self.menu }
    pub fn revision(&self) -> u64 { self.session.revision() }

    fn run(&mut self, events: &[Event]) -> Result<(), Error> {
        self.session.run_all(events)?;
        self.menu = self.session.view::<Menu>((north(),))?;
        Ok(())
    }

    pub fn open_history(&mut self, conversations: &[(&str, &str, &str)], query: &str, archived: bool) -> Result<(), Error> {
        let kind = if archived { "archived-history" } else { "history" };
        let mut events = vec![Event::ClearMenuRows { north: north() }, Event::OpenHistoryMenu { north: north(), kind: kind.into() }];
        for (key, label, description) in conversations {
            events.push(offer(key, label, description, ""));
        }
        events.push(Event::QueryMenu { north: north(), query: query.into() });
        self.run(&events)
    }

    pub fn open_settings_menu(&mut self, kind: &str, title: &str, rows: &[(&str, &str, &str, &str)]) -> Result<(), Error> {
        self.settings_menu(kind, title, rows, false)
    }

    pub fn work_menu(&mut self, rows: &[(&str, &str, &str, &str, &str, &str, bool)]) -> Result<(), Error> {
        let mut events = vec![Event::ClearMenuRows { north: north() }, Event::OpenSettingsMenu { north: north(), kind: "work".into(), title: "Work items".into() }];
        for (reference, title, state, holder, clock, eta, blocked) in rows {
            let description = if holder.is_empty() { "No holder".to_owned() } else { format!("{holder} · {clock} held / {eta} ETA") };
            let annotation = if *state == "closed" { "closed" } else if *blocked { "blocked" } else if holder.is_empty() { "ready" } else { "held" };
            events.push(offer(reference, &format!("{reference}  {title}"), &description, annotation));
        }
        events.push(Event::QueryMenu { north: north(), query: String::new() });
        self.run(&events)
    }

    pub fn finish_settings_effect(&mut self, kind: &str, title: &str, rows: &[(&str, &str, &str, &str)]) -> Result<(), Error> {
        self.settings_menu(kind, title, rows, true)
    }

    fn settings_menu(&mut self, kind: &str, title: &str, rows: &[(&str, &str, &str, &str)], finish_effect: bool) -> Result<(), Error> {
        let mut events = vec![Event::ClearMenuRows { north: north() }, Event::OpenSettingsMenu { north: north(), kind: kind.into(), title: title.into() }];
        for (key, label, description, annotation) in rows {
            events.push(offer(key, label, description, annotation));
        }
        events.push(Event::QueryMenu { north: north(), query: String::new() });
        if finish_effect { events.push(Event::ClearHostEffect { north: north() }); }
        self.run(&events)
    }

    pub fn query_menu(&mut self, query: &str) -> Result<(), Error> {
        self.run(&[Event::QueryMenu { north: north(), query: query.into() }])
    }

    pub fn move_menu(&mut self, delta: isize) -> Result<(), Error> {
        self.run(&[Event::MoveMenu { north: north(), delta: delta as i64 }])
    }

    pub fn close_menu(&mut self) -> Result<(), Error> { self.run(&[Event::CloseMenu { north: north() }]) }
    pub fn accept_menu(&mut self) -> Result<(), Error> { self.run(&[Event::AcceptMenu { north: north() }]) }
}
