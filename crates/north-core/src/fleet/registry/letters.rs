use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::relay::{self, Courier, Letter, Outbox, Relay, Relayed};
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::store::{FleetRow, RowKind, Spool, cached_sessions};

use crate::fleet::gates::GateRequest;

use super::{Fleet, parents};

const OUTBOX: &str = ".outbox";
const UNDELIVERED: &str = ".relay-undelivered";
const UNDROPPED: &str = ".relay-undropped";

pub enum Sent {
    Here,
    Posted { relay: Option<Relay>, letter: Letter },
}

pub enum Opened {
    Here(i64),
    Posted { relay: Relay, letter: Letter },
}

impl Fleet {
    /// Whether a message to `session` arrives: it lives here, or a relay carries it to the host that has it.
    pub fn reaches(&self, session: &str) -> NorthResult<()> {
        self.record(session).map(drop).or_else(|error| self.outbox().relay().map_or(Err(error), |_| Ok(())))
    }

    pub fn outbox(&self) -> Outbox {
        Outbox::new(self.root.join(OUTBOX))
    }

    pub fn spool(&self) -> Spool {
        Spool::at(&self.root)
    }

    pub fn send_or_post(&self, from: Option<&str>, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        if self.session_dir(name)?.exists() {
            return self.send_signed(from, name, text, delivery).map(|()| Sent::Here);
        }
        let known = match cached_sessions(&self.root)? {
            Some(sessions) => sessions.iter().any(|session| session.name == name),
            None => self.outbox().relay().is_some(),
        };
        if !known {
            return Err(NorthError::Usage(format!("no session named {name}")));
        }
        let row = self.spool_message(from, name, text, delivery)?;
        let letter = self.outbox().post(&row)?;
        Ok(Sent::Posted { relay: self.outbox().relay(), letter })
    }

    pub(super) fn spool_message(&self, from: Option<&str>, name: &str, text: &str, delivery: Delivery) -> NorthResult<FleetRow> {
        let sent_at = now_ms();
        let row = FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind: RowKind::Message,
            to: name.into(),
            from: relay::hostname(),
            urgent: delivery == Delivery::Now,
            sent_at,
            text: text.into(),
            sender: from.map(str::to_owned),
            gate: None,
            job: None,
        };
        Spool::at(&self.root).put(&row)?;
        Ok(row)
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
            None => self.hand_in(&FleetRow::from(letter)).map(drop),
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
    use crate::fleet::store::{SessionRow, Spool, cache_sessions};
    use crate::fleet::registry::HandedIn;
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

        fn revive_rebooted(&self, _: &str) -> NorthResult<Vec<String>> {
            Ok(Vec::new())
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

    struct Session {
        frames: Arc<std::sync::Mutex<Vec<String>>>,
    }

    fn live(fleet: &Fleet, name: &str) -> Session {
        use std::io::{BufRead, BufReader, Write};
        let dir = fleet.root.join(name);
        std::fs::create_dir_all(dir.join("g1")).unwrap();
        let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": name, "host_dir": dir, "stream_offset": 0});
        std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
        let assistant = serde_json::json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": "Not done: waiting."}]}});
        let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": name});
        std::fs::write(dir.join("g1/stream.jsonl"), format!("{assistant}\n{result}\n")).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(dir.join("host.sock")).unwrap();
        let frames = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = frames.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut writer = &stream;
                for line in BufReader::new(&stream).lines().map_while(Result::ok) {
                    seen.lock().unwrap().push(line);
                    writeln!(writer, "{{\"host_seq\":1}}").unwrap();
                }
            }
        });
        Session { frames }
    }

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "gex45".into(), status: "idle".into(), created: "t".into(), updated: "t".into() }).collect();
        cache_sessions(&fleet.root, &rows).unwrap();
    }

    #[test]
    fn a_send_to_a_session_here_spools_one_row_and_hands_it_in_once_however_often_the_row_is_handed_in_again() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");

        assert!(matches!(fleet.send_or_post(Some("lead"), "worker", "hello", Delivery::Queued).unwrap(), Sent::Here));
        let spooled = Spool::at(&fleet.root).pending().unwrap();

        assert_eq!(spooled.len(), 1);
        assert_eq!((spooled[0].to.as_str(), spooled[0].text.as_str(), spooled[0].sender.as_deref()), ("worker", "hello", Some("lead")));
        assert_eq!(fleet.hand_in(&spooled[0]).unwrap(), HandedIn::Already);
        assert_eq!(fleet.hand_in(&FleetRow::from(&Letter::from(&spooled[0]))).unwrap(), HandedIn::Already);
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("[from lead] hello"), "{}", frames[0]);
    }

    #[test]
    fn a_letter_for_a_row_already_handed_in_is_dropped_without_a_second_delivery() {
        let home = tempfile::tempdir().unwrap();
        let far_home = tempfile::tempdir().unwrap();
        let (laptop, far) = (fleet(home.path()), fleet(far_home.path()));
        let worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.outbox().take("laptop").unwrap();
        far.send_or_post(Some("lead"), "worker", "hello", Delivery::Queued).unwrap();
        let row = Spool::at(&far.root).pending().unwrap().remove(0);

        assert_eq!(laptop.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert!(worker.frames.lock().unwrap()[0].contains("[from lead@"), "a row from another host is signed with its host");
        let courier = Hosts(BTreeMap::from([("far".to_owned(), far.outbox())]));
        let (relayed, fresh) = laptop.relay_arming(&courier, &["far".to_owned()], &|_| Ok(())).unwrap();

        assert!(fresh.is_empty() && relayed.undelivered.is_empty(), "{:?}", relayed.lines());
        assert_eq!(worker.frames.lock().unwrap().len(), 1);
        assert!(far.outbox().take("laptop").unwrap().is_empty(), "the letter for the delivered row is dropped from the far host");
    }

    #[test]
    fn a_hand_in_that_fails_is_withdrawn_from_the_spool_and_the_row_can_be_handed_in_later() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");
        let socket = fleet.root.join("worker/host.sock");
        std::fs::rename(&socket, fleet.root.join("worker/host.moved")).unwrap();

        assert!(fleet.send_signed(None, "worker", "hello", Delivery::Queued).is_err());
        assert!(Spool::at(&fleet.root).pending().unwrap().is_empty());
        let row = fleet.spool_message(None, "worker", "again", Delivery::Queued).unwrap();
        assert!(fleet.hand_in(&row).is_err());
        std::fs::rename(fleet.root.join("worker/host.moved"), &socket).unwrap();

        assert_eq!(fleet.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(worker.frames.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_send_to_a_name_in_neither_the_registry_nor_the_cache_fails_at_once_and_spools_nothing() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        cached(&fleet, &["elsewhere"]);

        let refused = fleet.send_or_post(None, "nobody", "hello", Delivery::Queued);

        assert!(matches!(refused, Err(NorthError::Usage(why)) if why == "no session named nobody"));
        assert!(Spool::at(&fleet.root).pending().unwrap().is_empty() && fleet.outbox().take("laptop").unwrap().is_empty());
    }

    #[test]
    fn a_send_to_a_name_only_in_the_cache_is_spooled_and_posted_under_the_row_id() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        cached(&fleet, &["elsewhere"]);

        let Sent::Posted { relay, letter } = fleet.send_or_post(Some("lead"), "elsewhere", "hello", Delivery::Now).unwrap() else { panic!("a cached name is posted") };

        assert!(relay.is_none());
        let spooled = Spool::at(&fleet.root).pending().unwrap();
        assert_eq!(spooled.iter().map(|row| (row.id.as_str(), row.to.as_str(), row.urgent)).collect::<Vec<_>>(), [(letter.id.as_str(), "elsewhere", true)]);
        assert_eq!(fleet.outbox().take("laptop").unwrap(), [letter]);
    }

    #[test]
    fn a_host_whose_cache_was_never_written_posts_to_an_unknown_name_only_while_a_relay_takes_from_it() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());

        assert!(matches!(fleet.send_or_post(None, "elsewhere", "hello", Delivery::Queued), Err(NorthError::Usage(_))));
        fleet.outbox().take("laptop").unwrap();
        let Sent::Posted { relay, letter } = fleet.send_or_post(None, "elsewhere", "hello", Delivery::Queued).unwrap() else { panic!("a relayed host posts") };

        assert_eq!(relay.map(|relay| relay.host), Some("laptop".to_owned()));
        assert_eq!(Spool::at(&fleet.root).pending().unwrap().iter().map(|row| row.id.clone()).collect::<Vec<_>>(), [letter.id]);
    }
}
