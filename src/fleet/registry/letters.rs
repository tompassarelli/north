use crate::error::NorthResult;
use crate::fleet::model::Delivery;
use crate::fleet::relay::{self, Courier, Letter, Outbox, Relay, Relayed};

use super::Fleet;

const OUTBOX: &str = ".outbox";
const UNDELIVERED: &str = ".relay-undelivered";
const UNDROPPED: &str = ".relay-undropped";

pub enum Sent {
    Here,
    Posted { relay: Relay, letter: Letter },
}

impl Fleet {
    pub fn outbox(&self) -> Outbox {
        Outbox::new(self.root.join(OUTBOX))
    }

    pub fn send_or_post(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        if !self.session_dir(name)?.exists()
            && let Some(relay) = self.outbox().relay()
        {
            let letter = self.outbox().post(name, text, delivery)?;
            return Ok(Sent::Posted { relay, letter });
        }
        self.send(name, text, delivery).map(|()| Sent::Here)
    }

    pub fn relay(&self, courier: &dyn Courier, hosts: &[String]) -> NorthResult<(Relayed, Vec<String>)> {
        let undropped = self.root.join(UNDROPPED);
        let relayed = relay::relay(courier, &relay::hostname(), hosts, &relay::entries(&undropped), &mut |letter| self.send(&letter.to, &format!("[from {}] {}", letter.from, letter.text), letter.delivery()));
        let mut fresh = relayed.newly_undelivered(&self.root.join(UNDELIVERED))?;
        fresh.extend(relayed.newly_undropped(&undropped)?);
        Ok((relayed, fresh))
    }
}
