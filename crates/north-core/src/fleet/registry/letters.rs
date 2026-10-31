use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::relay::{self, Courier, Letter, Outbox, Relay, Relayed};
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::store::{FleetRow, RowKind, Spool, cached_sessions};

use super::Fleet;

const OUTBOX: &str = ".outbox";
const UNDELIVERED: &str = ".relay-undelivered";
const UNDROPPED: &str = ".relay-undropped";

pub enum Sent {
    Here,
    Posted { relay: Option<Relay>, letter: Letter },
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
        if !self.known_elsewhere(name)? {
            return Err(NorthError::Usage(format!("no session named {name}")));
        }
        let row = self.spool_message(from, name, text, delivery)?;
        let letter = self.outbox().post(&row)?;
        Ok(Sent::Posted { relay: self.outbox().relay(), letter })
    }

    pub(super) fn known_elsewhere(&self, name: &str) -> NorthResult<bool> {
        Ok(match cached_sessions(&self.root)? {
            Some(sessions) => sessions.iter().any(|session| session.name == name) || self.remote_views().iter().any(|seen| seen.snapshot.sessions.iter().any(|session| session.name == name)),
            None => self.outbox().relay().is_some(),
        })
    }

    pub(super) fn spool_message(&self, from: Option<&str>, name: &str, text: &str, delivery: Delivery) -> NorthResult<FleetRow> {
        let sent_at = now_ms();
        let row = FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind: RowKind::Message,
            to: name.into(),
            from: self.host.clone(),
            urgent: delivery == Delivery::Now,
            sent_at,
            text: text.into(),
            sender: from.map(str::to_owned),
            gate: None,
            answer: None,
            job: None,
        };
        Spool::at(&self.root).put(&row)?;
        Ok(row)
    }

    pub fn relay(&self, courier: &dyn Courier, hosts: &[String]) -> NorthResult<(Relayed, Vec<String>)> {
        let undropped = self.root.join(UNDROPPED);
        let relayed = relay::relay(courier, &relay::hostname(), hosts, &relay::entries(&undropped), &mut |letter| self.hand_in(&FleetRow::from(letter)).map(drop));
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

    use super::*;
    use super::super::fixtures::{fleet, live};
    use crate::fleet::remote_view::Snapshot;
    use crate::fleet::store::{SessionRow, Spool, cache_sessions};
    use crate::fleet::registry::HandedIn;
    use crate::error::NorthError;

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
    }

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "gex45".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
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
        let (laptop, far) = (fleet(home.path()).hosted_as("laptop"), fleet(far_home.path()).hosted_as("far"));
        let worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.outbox().take("laptop").unwrap();
        far.send_or_post(Some("lead"), "worker", "hello", Delivery::Queued).unwrap();
        let row = Spool::at(&far.root).pending().unwrap().remove(0);

        assert_eq!(laptop.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert!(worker.frames.lock().unwrap()[0].contains("[from lead@"), "a row from another host is signed with its host");
        let courier = Hosts(BTreeMap::from([("far".to_owned(), far.outbox())]));
        let (relayed, fresh) = laptop.relay(&courier, &["far".to_owned()]).unwrap();

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
    fn a_send_to_a_name_only_a_relayed_hosts_view_holds_is_posted_once_the_cache_exists() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        cached(&fleet, &["elsewhere"]);
        let view: crate::fleet::remote_view::SessionView = serde_json::from_value(serde_json::json!({"name": "legacy", "provider": "claude", "account": "b", "status": "parked", "parent": null, "cwd": "/tmp", "placement": null})).unwrap();
        fleet.remember("tom@gex45", &Snapshot { host: "gex45".into(), sessions: vec![view], gates: Vec::new() }).unwrap();

        let Sent::Posted { letter, .. } = fleet.send_or_post(None, "legacy", "hello", Delivery::Queued).unwrap() else { panic!("a name in a relayed view is posted") };

        assert_eq!(letter.to, "legacy");
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
