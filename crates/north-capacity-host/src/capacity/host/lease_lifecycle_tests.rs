use std::time::Duration;

use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Liveness, Owner, Store};
use serde_json::json;

use crate::capacity::lease_lifecycle::{DATABASE, Ledger, Leased, Phase};
use super::state::{ensure_state, lease_path, read_leases, write_lease};

const LEASE: &str = "6f1c9c3e-5b0a-4a52-9a53-0d7c1e9a4b21";

struct Dead(u32);

impl Liveness for Dead {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        Some(owner.pid != self.0)
    }
}

fn wrapper(pid: u32) -> Owner {
    Owner { host: "cpx62".to_owned(), pid, started: 7, booted_ms: 1 }
}

fn at(offset: Duration) -> Timestamp {
    Timestamp::from_unix_millis(Timestamp::now().unix_millis() + offset.as_millis() as i64)
}

#[test]
fn lease_lifecycle_refuses_a_move_out_of_a_reclaimed_lease_and_health_sees_the_attempt() {
    let root = tempfile::tempdir().unwrap();
    ensure_state(root.path());
    let expired = json!({ "id": LEASE, "kind": "lease", "owner": "tom", "createdAt": 1, "expiresAt": 2 });
    write_lease(&lease_path(root.path(), LEASE), &super::json::Obj::new().spread(&expired), true);

    let (active, reclaimed) = read_leases(root.path(), 1e12);

    assert_eq!((active.len(), reclaimed), (0, 1));
    assert!(!lease_path(root.path(), LEASE).exists());
    let mut ledger = Ledger::open(root.path()).unwrap();
    let ended = ledger.record(LEASE).unwrap().unwrap();
    assert_eq!((ended.state, ended.seq), (Phase::Reclaimed, 2));

    assert!(ledger.advance(LEASE, Phase::Frozen, "a thaw tick that read a stale list", None).unwrap().is_none());

    let record = ledger.record(LEASE).unwrap().unwrap();
    assert_eq!((record.state, record.seq), (Phase::Reclaimed, 2));
    let faults = Store::open(&root.path().join(DATABASE)).unwrap().faults(at(Duration::ZERO)).unwrap();
    assert_eq!(faults.len(), 1);
    assert!(matches!(&faults[0].kind, FaultKind::Illegal { to, .. } if to == "frozen"));
}

#[test]
fn lease_lifecycle_reports_a_lease_left_frozen_past_its_dwell_and_a_wrapper_that_died_settling() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(DATABASE);
    let mut thaw_stopped = Store::open_as(&path, wrapper(10), Box::new(Dead(0))).unwrap();
    thaw_stopped.transition::<Leased>("frozen-lease", Phase::Running, "admitted as a run lease", Timestamp::now()).unwrap().unwrap();
    thaw_stopped.transition::<Leased>("frozen-lease", Phase::Frozen, "a gaming hold froze its scope", Timestamp::now()).unwrap().unwrap();
    thaw_stopped.transition::<Leased>("settling-lease", Phase::Running, "admitted as a run lease", Timestamp::now()).unwrap().unwrap();
    thaw_stopped.transition::<Leased>("settling-lease", Phase::Settling, "the command exited with 0", Timestamp::now()).unwrap().unwrap();

    let healthy = thaw_stopped.faults(at(Duration::from_secs(60))).unwrap();
    assert!(healthy.is_empty(), "{healthy:?}");

    let reader = Store::open_as(&path, wrapper(11), Box::new(Dead(10))).unwrap();
    let faults = reader.faults(at(Duration::from_secs(7 * 3600))).unwrap();
    let kinds: Vec<_> = faults.iter().map(|fault| (fault.instance.as_str(), fault.state.as_str(), matches!(fault.kind, FaultKind::Overdue { .. }), matches!(fault.kind, FaultKind::Orphaned { .. }))).collect();
    assert_eq!(kinds, [("frozen-lease", "frozen", true, false), ("settling-lease", "settling", false, true)]);
}
