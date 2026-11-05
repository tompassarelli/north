use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::sender::Sender;
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
            sender: Sender::typed(self.caller()),
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
            ControlVerb::Revive => self.revive(name, None).map(drop),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live};
    use super::*;
    use crate::fleet::store::{SessionRow, Spool, cache_sessions};

    #[test]
    fn a_revive_of_a_session_another_host_holds_is_spooled_and_applied_by_that_host_once() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let _worker = live(&laptop, "worker");
        let rows = [SessionRow { name: "worker".into(), host: "laptop".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }];
        cache_sessions(&far.root, &rows).unwrap();

        assert!(matches!(far.control(ControlVerb::Revive, "nobody"), Err(NorthError::Usage(why)) if why == "no session named nobody"));
        let Controlled::Spooled(row) = far.control(ControlVerb::Revive, "worker").unwrap() else { panic!("a session of another host is spooled") };
        let pending = Spool::at(&far.root).pending().unwrap();
        assert_eq!((pending.len(), row.kind, row.text.as_str()), (1, RowKind::Control, "revive"));

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);
    }
}
