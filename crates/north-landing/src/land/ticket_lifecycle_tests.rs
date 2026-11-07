use std::ffi::OsStr;

use north_base::foundation::clock::Timestamp;
use north_base::land_ticket::{self, LandTicket, Phase, Records, queued};
use north_base::lifecycle::{FaultKind, Owner, Proc, Store};

use super::{self as queue, Ticket};

fn ticket(branch: &str) -> Ticket {
    Ticket { branch: branch.to_owned(), queued_micros: Some(1_000), ..Default::default() }
}

#[test]
fn a_landed_ticket_refuses_a_later_check_and_keeps_its_record() {
    let queue_dir = tempfile::tempdir().unwrap();
    let path = queue_dir.path().join(queue::file_name(1_000, "lane"));
    queue::write(&path, &ticket("lane")).unwrap();
    queued(&path, "enqueued").unwrap();
    Records::of(&path).unwrap().checking(std::slice::from_ref(&path)).unwrap();
    let decided = queue::decide(&path, ticket("lane"), "landed abc123".to_owned()).unwrap();
    let store = land_ticket::open(queue_dir.path()).unwrap();
    let instance = land_ticket::instance(queue_dir.path(), OsStr::new(path.file_name().unwrap()));
    let landed = store.current::<LandTicket>(&instance).unwrap().unwrap();

    let refused = Records::of(&path).unwrap().checking(std::slice::from_ref(&path));

    assert!(refused.is_err(), "a stale lander must not check a landed ticket again");
    assert_eq!(store.current::<LandTicket>(&instance).unwrap().unwrap(), landed);
    assert_eq!(landed.state, Phase::Landed);
    assert!(decided.starts_with(queue_dir.path().join("done")) && !path.exists());
    let faults = store.faults(Timestamp::now()).unwrap();
    assert!(faults.iter().any(|fault| fault.instance == instance && matches!(&fault.kind, FaultKind::Illegal { to, .. } if to == "checking")), "{faults:?}");
}

// KEEP(bug): a lander killed mid-check left its tickets in the queue directory with nothing saying they were held, so the lost check never showed in health (north#757 entry 2)
#[test]
fn a_ticket_whose_lander_died_mid_check_is_orphaned_in_health_and_queued_again_by_the_next_lander() {
    let queue_dir = tempfile::tempdir().unwrap();
    let path = queue_dir.path().join(queue::file_name(1_000, "lane"));
    queue::write(&path, &ticket("lane")).unwrap();
    queued(&path, "enqueued").unwrap();
    let instance = land_ticket::instance(queue_dir.path(), OsStr::new(path.file_name().unwrap()));
    let dead = Owner { started: u64::MAX, ..Owner::current() };
    let mut killed = Store::open_as(&land_ticket::ledger(queue_dir.path()), dead.clone(), Box::new(Proc)).unwrap();
    killed.transition::<LandTicket>(&instance, Phase::Checking, "a land check took it", Timestamp::now()).unwrap().unwrap();

    let faults = land_ticket::open(queue_dir.path()).unwrap().faults(Timestamp::now()).unwrap();
    assert!(faults.iter().any(|fault| fault.instance == instance && fault.state == "checking" && fault.kind == FaultKind::Orphaned { owner: dead.clone() }), "{faults:?}");

    queue::adopt(queue_dir.path()).unwrap();

    let store = land_ticket::open(queue_dir.path()).unwrap();
    assert_eq!(store.current::<LandTicket>(&instance).unwrap().unwrap().state, Phase::Queued);
    assert!(store.faults(Timestamp::now()).unwrap().is_empty());
    assert_eq!(queue::collect(queue_dir.path()).unwrap().len(), 1, "the next lander checks the ticket again");
}
