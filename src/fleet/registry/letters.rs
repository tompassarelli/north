use crate::error::NorthResult;
use crate::fleet::model::Delivery;
use crate::fleet::relay::{self, Courier, Letter, Outbox, Relay, Relayed};

use crate::fleet::gates::GateRequest;

use super::{Fleet, parents};

const OUTBOX: &str = ".outbox";
const UNDELIVERED: &str = ".relay-undelivered";
const UNDROPPED: &str = ".relay-undropped";

pub enum Sent {
    Here,
    Posted { relay: Relay, letter: Letter },
}

pub enum Opened {
    Here(i64),
    Posted { relay: Relay, letter: Letter },
}

impl Fleet {
    pub fn outbox(&self) -> Outbox {
        Outbox::new(self.root.join(OUTBOX))
    }

    pub fn send_or_post(&self, from: Option<&str>, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        if !self.session_dir(name)?.exists()
            && let Some(relay) = self.outbox().relay()
        {
            let letter = self.outbox().post(from, name, text, delivery)?;
            return Ok(Sent::Posted { relay, letter });
        }
        self.send_signed(from, name, text, delivery).map(|()| Sent::Here)
    }

    pub fn open_or_post_gate(&self, request: &GateRequest) -> NorthResult<Opened> {
        if !self.session_dir(&request.session)?.exists()
            && let Some(relay) = self.outbox().relay()
        {
            let letter = self.outbox().post_gate(request)?;
            return Ok(Opened::Posted { relay, letter });
        }
        self.open_gate(request).map(Opened::Here)
    }

    pub fn relay(&self, courier: &dyn Courier, hosts: &[String]) -> NorthResult<(Relayed, Vec<String>)> {
        self.relay_arming(courier, hosts, &parents::await_default)
    }

    pub(super) fn relay_arming(&self, courier: &dyn Courier, hosts: &[String], arm: &dyn Fn(i64) -> NorthResult<()>) -> NorthResult<(Relayed, Vec<String>)> {
        let undropped = self.root.join(UNDROPPED);
        let relayed = relay::relay(courier, &relay::hostname(), hosts, &relay::entries(&undropped), &mut |letter| match &letter.gate {
            Some(request) => self.open_gate_arming(request, arm).map(drop),
            None => self.send_signed(Some(&letter.signer()), &letter.to, &letter.text, letter.delivery()),
        });
        for (ssh, snapshot) in &relayed.snapshots {
            self.remember(ssh, snapshot)?;
        }
        let mut fresh = relayed.newly_undelivered(&self.root.join(UNDELIVERED))?;
        fresh.extend(relayed.newly_undropped(&undropped)?);
        Ok((relayed, fresh))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::fleet::remote_view::Snapshot;
    use crate::error::NorthError;
    use crate::fleet::runtimes::Runtimes;
    use crate::fleet::records::write_record;
    use crate::fleet::model::SessionRecord;

    fn fleet(home: &std::path::Path) -> Fleet {
        let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
        Fleet::at(home.into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap()
    }

    struct Hosts(BTreeMap<String, Outbox>);

    impl Courier for Hosts {
        fn take(&self, host: &str, relay_host: &str) -> NorthResult<Vec<Letter>> {
            self.0[host].take(relay_host)
        }

        fn drop(&self, host: &str, ids: &[String]) -> NorthResult<()> {
            self.0[host].drop(ids)
        }

        fn snapshot(&self, _: &str) -> NorthResult<Snapshot> {
            Ok(Snapshot::default())
        }

        fn answer(&self, host: &str, _: Option<&str>, _: i64, _: usize, _: Option<&str>) -> NorthResult<String> {
            Err(NorthError::Usage(format!("ssh {host}: unreachable")))
        }
    }

    #[test]
    fn a_gate_opened_on_the_host_that_does_not_register_its_session_is_owned_by_that_session_on_the_launching_host() {
        let laptop_home = tempfile::tempdir().unwrap();
        let far_home = tempfile::tempdir().unwrap();
        let laptop = fleet(laptop_home.path());
        let far = fleet(far_home.path());
        let record: SessionRecord = serde_json::from_value(serde_json::json!({
            "name": "sc-413-d1936", "provider": "claude", "account": "a", "cwd": "/tmp",
            "model": null, "created_ms": 1, "host_pid": null, "conversation": "c0ffee",
        }))
        .unwrap();
        let session_dir = laptop.session_dir("sc-413-d1936").unwrap();
        std::fs::create_dir_all(&session_dir).unwrap();
        write_record(&session_dir, &record).unwrap();
        let request = GateRequest { verification: false, issue: None, session: "sc-413-d1936".into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 60_000 };

        far.outbox().take("laptop").unwrap();
        assert!(matches!(far.open_or_post_gate(&request).unwrap(), Opened::Posted { .. }), "a session unknown here posts its gate to the relay");

        let courier = Hosts(BTreeMap::from([("far".to_owned(), far.outbox())]));
        let (relayed, fresh) = laptop.relay_arming(&courier, &["far".to_owned()], &|_| Ok(())).unwrap();
        assert!(fresh.is_empty() && relayed.undelivered.is_empty(), "{:?}", relayed.lines());
        let gates = laptop.digest(0).unwrap().gates;
        assert_eq!(gates.iter().map(|gate| (gate.request.session.as_str(), gate.request.question.as_str(), gate.is_open())).collect::<Vec<_>>(), [("sc-413-d1936", "Rent the box?", true)]);
        assert!(far.outbox().take("laptop").unwrap().is_empty(), "the relayed gate letter is dropped from the far host");
    }
}
