use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::store::{ControlVerb, FleetRow, RowKind, cached_sessions};

use super::{Fleet, HandedIn};

pub enum Controlled {
    Here,
    Spooled(Box<FleetRow>),
}

impl Fleet {
    pub fn control(&self, verb: ControlVerb, name: &str) -> NorthResult<Controlled> {
        if self.session_dir(name)?.exists() {
            return self.run_control(verb, name).map(|()| Controlled::Here);
        }
        let known = cached_sessions(&self.root)?.is_some_and(|sessions| sessions.iter().any(|session| session.name == name));
        if !known {
            return Err(NorthError::Usage(format!("no session named {name}")));
        }
        let sent_at = now_ms();
        let row = FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind: RowKind::Control,
            to: name.into(),
            from: self.host.clone(),
            urgent: false,
            sent_at,
            text: verb.label().into(),
            sender: None,
            gate: None,
            answer: None,
            job: None,
            origin: None,
        };
        self.spool().put(&row)?;
        Ok(Controlled::Spooled(Box::new(row)))
    }

    pub(super) fn apply_control(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let verb = ControlVerb::parse(&row.text)?;
        row.validate()?;
        match verb {
            ControlVerb::Forget => self.forget(&row.to).map(|()| HandedIn::Delivered),
            ControlVerb::Interrupt if !self.running(&self.record(&row.to)?) => self.journaled(row, || Ok(())),
            verb => self.journaled(row, || self.run_control(verb, &row.to)),
        }
    }

    fn run_control(&self, verb: ControlVerb, name: &str) -> NorthResult<()> {
        match verb {
            ControlVerb::Forget => self.forget(name),
            ControlVerb::Stop => self.stop(name),
            ControlVerb::Interrupt => self.interrupt(name),
        }
    }
}
