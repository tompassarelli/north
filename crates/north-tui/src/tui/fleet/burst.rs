//! Typed bursts: an Enter that another event follows within the burst gap is a newline inside a burst, not a send.

use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::error::NorthResult;

pub const BURST_GAP: Duration = Duration::from_millis(15);

type Source<'a> = dyn FnMut(Duration) -> NorthResult<Option<Event>> + 'a;

pub struct Bursts<'a, 's> {
    source: &'s mut Source<'a>,
    held: Option<Event>,
}

impl<'a, 's> Bursts<'a, 's> {
    pub fn new(source: &'s mut Source<'a>) -> Self {
        Self { source, held: None }
    }

    pub fn next(&mut self, timeout: Duration) -> NorthResult<Option<Event>> {
        let event = match self.held.take() {
            Some(held) => Some(held),
            None => (self.source)(timeout)?,
        };
        let Some(Event::Key(key)) = event else { return Ok(event) };
        if !is_plain_enter(key) {
            return Ok(Some(Event::Key(key)));
        }
        Ok(match (self.source)(BURST_GAP)? {
            Some(next) => {
                self.held = Some(next);
                Some(Event::Key(KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::SHIFT, key.kind)))
            }
            None => Some(Event::Key(key)),
        })
    }
}

fn is_plain_enter(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter && key.modifiers.is_empty() && key.kind != KeyEventKind::Release
}
