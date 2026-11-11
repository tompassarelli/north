use north_base::lifecycle::{Refusal, Store};
use north_session_host::row_delivery::{Phase, RowDelivery, adopt, instance};

use crate::error::{NorthError, NorthResult};
use crate::fleet::store::FleetRow;
use crate::foundation::clock::Timestamp;

use super::Fleet;

impl Fleet {
    pub(super) fn deliveries(&self) -> NorthResult<Store> {
        Store::open(&self.state.join("ledger.db"))
    }

    pub(super) fn advance(&self, store: &mut Store, session: &str, id: &str, to: Phase, cause: &str) -> NorthResult<bool> {
        match store.transition::<RowDelivery>(&instance(session, id), to, cause, Timestamp::now())? {
            Ok(_) => Ok(true),
            Err(Refusal::Held { .. }) => Ok(false),
            Err(refusal) => Err(NorthError::State(format!("delivery of row {id} to {session} was refused: {refusal:?}"))),
        }
    }

    pub fn delivered(&self, name: &str) -> NorthResult<Vec<String>> {
        let prefix = instance(name, "");
        Ok(self.deliveries()?.instances_under::<RowDelivery>(&prefix)?.into_iter().filter(|record| record.state == Phase::Delivered).filter_map(|record| record.instance.strip_prefix(&prefix).map(str::to_owned)).collect())
    }

    pub fn prune_delivered(&self, before: std::time::SystemTime) -> NorthResult<usize> {
        self.deliveries()?.forget::<RowDelivery>(Timestamp::from(before))
    }

    /// Adopts each session's legacy delivery journal, found by its directory alone, since every fleet open runs this and reading each record made every open read the whole registry.
    pub(super) fn adopt_journals(&self) -> NorthResult<()> {
        for entry in std::fs::read_dir(&self.root)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(journal) = self.session_dir(&name).map(|dir| dir.join("delivered")) else { continue };
            if journal.is_dir() {
                adopt(&mut self.deliveries()?, &name, &journal, |id| self.took(&name, id), |id| FleetRow::check_id(id).is_ok())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod delivery_lifecycle {
    use north_base::lifecycle::{FaultKind, Lifecycle, Owner, Proc};

    use super::super::fixtures::{fleet, live};
    use super::*;
    use crate::fleet::model::Delivery;
    use crate::fleet::registry::HandedIn;
    use crate::fleet::sender::Sender;

    #[test]
    fn an_illegal_move_is_refused_and_a_delivered_row_cannot_be_handed_in_again() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");
        let row = fleet.spool_message(&Sender::Tom, "worker", "hello", Delivery::Queued).unwrap();
        assert_eq!(fleet.hand_in(&row).unwrap(), HandedIn::Delivered);

        let mut store = fleet.deliveries().unwrap();
        let refused = store.transition::<RowDelivery>(&instance("worker", &row.id), Phase::HandingIn, "a writer that skipped the table", Timestamp::now()).unwrap();

        assert_eq!(refused, Err(Refusal::Illegal { from: Some(Phase::Delivered), to: Phase::HandingIn }));
        assert_eq!(store.current::<RowDelivery>(&instance("worker", &row.id)).unwrap().unwrap().state, Phase::Delivered);
        assert_eq!(fleet.hand_in(&row).unwrap(), HandedIn::Already);
        assert_eq!(worker.frames.lock().unwrap().len(), 1);
        let faults = store.faults(Timestamp::now()).unwrap();
        assert!(faults.iter().any(|fault| fault.instance == instance("worker", &row.id) && matches!(&fault.kind, FaultKind::Illegal { to, .. } if to == "handing-in")), "{faults:?}");
    }

    #[test]
    fn a_hand_in_killed_mid_delivery_is_reported_orphaned_and_the_next_hand_in_logs_its_death_and_delivers() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");
        let row = fleet.spool_message(&Sender::Tom, "worker", "hello", Delivery::Queued).unwrap();
        let dead = Owner { host: crate::platform::host::name(), pid: u32::MAX - 1, started: 1, booted_ms: crate::platform::host::booted_ms().unwrap_or(0) };
        let mut killed = Store::open_as(&fleet.state.join("ledger.db"), dead.clone(), Box::new(Proc)).unwrap();
        killed.transition::<RowDelivery>(&instance("worker", &row.id), Phase::HandingIn, "handing the row to the session", Timestamp::now()).unwrap().unwrap();

        let faults = fleet.deliveries().unwrap().faults(Timestamp::now()).unwrap();
        assert!(faults.iter().any(|fault| fault.instance == instance("worker", &row.id) && fault.kind == FaultKind::Orphaned { owner: dead.clone() }), "{faults:?}");

        assert_eq!(fleet.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(worker.frames.lock().unwrap().len(), 1);
        let store = fleet.deliveries().unwrap();
        assert_eq!(store.current::<RowDelivery>(&instance("worker", &row.id)).unwrap().unwrap().state, Phase::Delivered);
        let moves: Vec<_> = store.history(RowDelivery::NAME, &instance("worker", &row.id)).unwrap().into_iter().map(|logged| (logged.from, logged.to, logged.cause)).collect();
        assert_eq!(moves[1], (Some("handing-in".into()), "interrupted".into(), format!("the holder, {dead}, is gone")));
        assert!(store.faults(Timestamp::now()).unwrap().is_empty());
    }
}
