use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Delivery, Status};
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind, SessionRow, Spool, cached_sessions};

use super::Fleet;

#[derive(Debug, Eq, PartialEq)]
pub enum Sent {
    HandedIn,
    Spooled { id: String, host: String },
}

impl Fleet {
    pub fn reaches(&self, session: &str) -> NorthResult<()> {
        if self.session_dir(session)?.exists() || self.held_elsewhere(session)?.is_some() {
            return Ok(());
        }
        Err(NorthError::Usage(format!("no session named {session}")))
    }

    pub fn spool(&self) -> Spool {
        Spool::at(&self.root)
    }

    pub fn send_message(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        if self.session_dir(name)?.exists() {
            return self.send_signed(from, name, text, delivery).map(|()| Sent::HandedIn);
        }
        let held = self.held_elsewhere(name)?.ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?;
        let row = self.spool_message(from, name, text, delivery)?;
        Ok(Sent::Spooled { id: row.id, host: held.host })
    }

    pub(super) fn known_elsewhere(&self, name: &str) -> NorthResult<bool> {
        Ok(self.held_elsewhere(name)?.is_some())
    }

    pub(super) fn held_elsewhere(&self, name: &str) -> NorthResult<Option<SessionRow>> {
        Ok(cached_sessions(&self.root)?.unwrap_or_default().into_iter().find(|session| session.name == name && session.host != self.host && session.status != Status::Exited.label()))
    }

    pub(super) fn spool_message(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<FleetRow> {
        let sent_at = now_ms();
        let row = FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind: RowKind::Message,
            to: name.into(),
            from: self.host.clone(),
            urgent: delivery == Delivery::Now,
            sent_at,
            text: text.into(),
            sender: from.clone(),
            gate: None,
            answer: None,
            job: None,
            origin: None,
        };
        Spool::at(&self.root).put(&row)?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixtures::{fleet, live};
    use crate::fleet::store::cache_sessions;
    use crate::fleet::registry::HandedIn;

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "far-host".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
        cache_sessions(&fleet.root, &rows).unwrap();
    }

    #[test]
    fn a_send_to_a_session_here_spools_one_row_and_hands_it_in_once_however_often_the_row_is_handed_in_again() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");

        assert_eq!(fleet.send_message(&Sender::Session("lead".into()), "worker", "hello", Delivery::Queued).unwrap(), Sent::HandedIn);
        let spooled = Spool::at(&fleet.root).pending().unwrap();

        assert_eq!(spooled.len(), 1);
        assert_eq!((spooled[0].to.as_str(), spooled[0].text.as_str(), &spooled[0].sender), ("worker", "hello", &Sender::Session("lead".into())));
        assert_eq!(fleet.hand_in(&spooled[0]).unwrap(), HandedIn::Already);
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("[from lead] hello"), "{}", frames[0]);
    }

    #[test]
    fn a_message_from_tom_reaches_the_session_unprefixed_and_any_other_sender_is_named() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");

        fleet.send_signed(&Sender::Tom, "worker", "ship it", Delivery::Queued).unwrap();
        fleet.send_signed(&Sender::Service("north-ops-check".into()), "worker", "tick", Delivery::Queued).unwrap();
        fleet.send_signed(&Sender::Session("lead".into()), "worker", "also", Delivery::Queued).unwrap();

        let rows = Spool::at(&fleet.root).pending().unwrap();
        for sender in [Sender::Tom, Sender::Service("north-ops-check".into()), Sender::Session("lead".into())] {
            assert!(rows.iter().any(|row| row.sender == sender), "{sender:?}");
        }
        let frames = worker.frames.lock().unwrap();
        let frame = |text: &str| frames.iter().find(|frame| frame.contains(text)).unwrap_or_else(|| panic!("{text}: {frames:?}")).clone();
        assert!(!frame("ship it").contains("[from"), "{frames:?}");
        assert!(frame("tick").contains("[from service:north-ops-check] tick"));
        assert!(frame("also").contains("[from lead] also"));
    }

    #[test]
    fn tom_on_another_host_is_still_unprefixed_where_a_service_names_its_host() {
        let home = tempfile::tempdir().unwrap();
        let far_home = tempfile::tempdir().unwrap();
        let (laptop, far) = (fleet(home.path()).hosted_as("laptop"), fleet(far_home.path()).hosted_as("far"));
        let worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.send_message(&Sender::Tom, "worker", "from the TUI", Delivery::Queued).unwrap();
        far.send_message(&Sender::Service("tick".into()), "worker", "from a timer", Delivery::Queued).unwrap();
        for row in Spool::at(&far.root).pending().unwrap() {
            laptop.hand_in(&row).unwrap();
        }
        let frames = worker.frames.lock().unwrap();
        assert!(frames.iter().any(|frame| frame.contains("from the TUI") && !frame.contains("[from")), "{frames:?}");
        assert!(frames.iter().any(|frame| frame.contains("[from service:tick@far] from a timer")), "{frames:?}");
    }

    #[test]
    fn a_row_sent_on_another_host_is_handed_in_signed_with_that_host_and_only_once() {
        let home = tempfile::tempdir().unwrap();
        let far_home = tempfile::tempdir().unwrap();
        let (laptop, far) = (fleet(home.path()).hosted_as("laptop"), fleet(far_home.path()).hosted_as("far"));
        let worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.send_message(&Sender::Session("lead".into()), "worker", "hello", Delivery::Queued).unwrap();
        let row = Spool::at(&far.root).pending().unwrap().remove(0);

        assert_eq!(laptop.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.hand_in(&row).unwrap(), HandedIn::Already);
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("[from lead@far] hello"), "{}", frames[0]);
    }

    #[test]
    fn a_hand_in_that_fails_is_withdrawn_from_the_spool_and_the_row_can_be_handed_in_later() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");
        let socket = fleet.root.join("worker/host.sock");
        std::fs::rename(&socket, fleet.root.join("worker/host.moved")).unwrap();

        assert!(fleet.send_signed(&Sender::Tom, "worker", "hello", Delivery::Queued).is_err());
        assert!(Spool::at(&fleet.root).pending().unwrap().is_empty());
        let row = fleet.spool_message(&Sender::Tom, "worker", "again", Delivery::Queued).unwrap();
        assert!(fleet.hand_in(&row).is_err());
        std::fs::rename(fleet.root.join("worker/host.moved"), &socket).unwrap();

        assert_eq!(fleet.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(worker.frames.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_send_to_a_name_in_neither_the_registry_nor_the_cache_fails_at_once_and_spools_nothing() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());

        assert!(matches!(fleet.send_message(&Sender::Tom, "nobody", "hello", Delivery::Queued), Err(NorthError::Usage(why)) if why == "no session named nobody"), "a host whose cache was never written knows no other host's names");
        cached(&fleet, &["elsewhere"]);
        assert!(matches!(fleet.send_message(&Sender::Tom, "nobody", "hello", Delivery::Queued), Err(NorthError::Usage(why)) if why == "no session named nobody"));
        assert!(fleet.reaches("nobody").is_err());
        assert!(Spool::at(&fleet.root).pending().unwrap().is_empty());
    }

    #[test]
    fn a_send_to_a_name_the_cache_lists_as_exited_or_as_this_hosts_own_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path()).hosted_as("laptop");
        let row = |name: &str, host: &str, status: &str| SessionRow { name: name.into(), host: host.into(), status: status.into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() };
        cache_sessions(&fleet.root, &[row("gone", "far-host", "exited"), row("forgotten", "laptop", "idle")]).unwrap();

        for name in ["gone", "forgotten"] {
            assert!(matches!(fleet.send_message(&Sender::Tom, name, "hello", Delivery::Queued), Err(NorthError::Usage(_))), "{name}");
            assert!(fleet.reaches(name).is_err(), "{name}");
        }
        assert!(Spool::at(&fleet.root).pending().unwrap().is_empty());
    }

    #[test]
    fn a_send_to_a_name_only_the_cache_holds_is_spooled_for_the_tick_and_names_its_host() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path()).hosted_as("laptop");
        cached(&fleet, &["elsewhere"]);

        let Sent::Spooled { id, host } = fleet.send_message(&Sender::Session("lead".into()), "elsewhere", "hello", Delivery::Now).unwrap() else { panic!("a cached name is spooled") };

        assert_eq!(host, "far-host");
        let spooled = Spool::at(&fleet.root).pending().unwrap();
        assert_eq!(spooled.iter().map(|row| (row.id.as_str(), row.to.as_str(), row.urgent)).collect::<Vec<_>>(), [(id.as_str(), "elsewhere", true)]);
        assert!(fleet.reaches("elsewhere").is_ok());
    }
}
