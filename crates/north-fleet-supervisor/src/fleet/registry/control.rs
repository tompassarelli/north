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
}

impl Fleet {
    pub fn control(&self, verb: ControlVerb, name: &str) -> NorthResult<Controlled> {
        if self.session_dir(name)?.exists() {
            return self.run_control(verb, name, self.caller().as_deref()).map(|()| Controlled::Here);
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
            ControlVerb::Forget => self.forget_for(&row.to, self.ordered_by(row.sender.session(), Halt::Ordered)).map(|()| HandedIn::Delivered),
            ControlVerb::Stop if let Some(refusal) = self.stop_refusal(&row.to)? => {
                eprintln!("north fleet: {refusal}");
                self.journaled(row, || Ok(()))
            }
            ControlVerb::Interrupt if !self.running(&self.record(&row.to)?) => self.journaled(row, || Ok(())),
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

