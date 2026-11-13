use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::{ControlVerb, FleetRow, RowKind, cached_sessions};

use super::relaunching::REVIVED_MID_TURN;
use super::stopping::Halt;
use super::{Fleet, HandedIn};

pub enum Controlled {
    Here,
    Spooled(Box<FleetRow>),
    Retired { host: String },
    Withdrawn,
}

impl Fleet {
    pub fn control(&self, verb: ControlVerb, name: &str) -> NorthResult<Controlled> {
        if self.session_dir(name)?.exists() {
            return self.run_control(verb, name, self.caller().as_deref()).map(|()| Controlled::Here);
        }
        let Some(held) = cached_sessions(&self.root)?.and_then(|sessions| sessions.into_iter().find(|session| session.name == name)) else {
            return Err(NorthError::Usage(format!("no session named {name}")));
        };
        if verb != ControlVerb::Revive && self.placed_hosts().is_some_and(|placed| !placed.contains(&held.host)) {
            self.registrar.retire_unplaced()?;
            if verb == ControlVerb::Forget {
                self.release_all_claims_of(name)?;
            }
            self.uncache_session(name)?;
            return Ok(Controlled::Retired { host: held.host });
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
            spooled_by: None,
        };
        self.spool().put(&row)?;
        Ok(Controlled::Spooled(Box::new(row)))
    }

    pub(super) fn apply_control(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let verb = ControlVerb::parse(&row.text)?;
        row.validate()?;
        if let Some(writer) = row.spooled_by.as_deref()
            && self.record(&row.to).ok().and_then(|record| record.parent).as_deref() != Some(writer)
        {
            eprintln!("north fleet: refused CONTROL row {} from {writer}'s spool: {} is not its child", row.id, row.to);
            return self.journaled(row, || Ok(()));
        }
        match verb {
            ControlVerb::Forget => self.forget_for(&row.to, self.ordered_by(row.sender.session(), Halt::Ordered)).map(|()| HandedIn::Delivered),
            ControlVerb::Stop if let Some(refusal) = self.stop_refusal(&row.to)? => {
                eprintln!("north fleet: {refusal}");
                self.journaled(row, || Ok(()))
            }
            ControlVerb::Interrupt if !self.record(&row.to)?.live() => self.journaled(row, || Ok(())),
            verb => self.journaled(row, || self.run_control(verb, &row.to, row.sender.session())),
        }
    }

    /// Runs `verb` on `name` as `caller` ordered it, so a stop or interrupt a proxy ordered records the proxy as its cause.
    pub(super) fn run_control(&self, verb: ControlVerb, name: &str, caller: Option<&str>) -> NorthResult<()> {
        let why = self.ordered_by(caller, Halt::Ordered);
        match verb {
            ControlVerb::Forget => self.forget_for(name, why),
            ControlVerb::Stop => match self.stop_refusal(name)? {
                Some(refusal) => Err(NorthError::Refused(refusal)),
                None => self.halt(name, why),
            },
            ControlVerb::Interrupt => self.interrupt_for(name, why),
            ControlVerb::Revive => {
                let cut_short = self.left_mid_turn(name)?;
                self.revive(name, None)?;
                if cut_short { self.carry_on(name, REVIVED_MID_TURN) } else { Ok(()) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use super::super::fixtures::fleet;
    use super::*;
    use crate::fleet::claims::Rows;
    use crate::fleet::store::SessionRow;

    #[test]
    fn a_forget_of_a_session_whose_host_left_placement_releases_the_claims_it_holds() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let policy = crate::platform::orchestration::under(home.path());
        fs::create_dir_all(policy.parent().unwrap()).unwrap();
        fs::write(&policy, "[hosts.here]\n").unwrap();
        let owned = BTreeSet::from(["n870-gone".to_owned()]);
        fleet.claims.hold("north#870@the first box", "n870-gone", &Default::default()).unwrap();
        fleet.claims.hold("north#870@the second box", "n870-gone", &Default::default()).unwrap();
        fleet
            .cache_sessions(&[SessionRow {
                name: "n870-gone".into(),
                host: "gone".into(),
                status: "exited".into(),
                created: String::new(),
                updated: String::new(),
                provider: "claude".into(),
                account: "a".into(),
                cwd: "/tmp".into(),
                parent: None,
                placement: None,
                machine: None,
                tasks: Vec::new(),
                conversation: None,
                predicted: Vec::new(),
            }])
            .unwrap();

        let forgotten = fleet.forget_unless_queued("n870-gone", None, None).unwrap();

        assert!(matches!(forgotten.controlled, Controlled::Retired { ref host } if host == "gone"));
        assert!(fleet.claims.holders(Rows::Owners(&owned)).unwrap().is_empty(), "a forgotten session leaves no claim in the ledger even when the host that ran it is no longer placed");
    }
}
