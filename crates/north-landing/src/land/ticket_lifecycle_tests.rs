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

// KEEP(bug): queue::requeue had no caller for a quarantined ticket, so the lander watchdog's quarantine had no exit (north#757 entry 2)
#[test]
fn a_quarantined_ticket_is_queued_again_by_name_and_a_queued_one_is_not() {
    let queue_dir = tempfile::tempdir().unwrap();
    let path = queue_dir.path().join(queue::file_name(1_000, "lane"));
    queue::write(&path, &ticket("lane")).unwrap();
    queued(&path, "enqueued").unwrap();
    let quarantined = queue::settle(&path, ticket("lane"), "quarantined by the lander watchdog".to_owned(), Phase::Quarantined).unwrap();
    assert!(quarantined.starts_with(queue_dir.path().join("quarantine")));

    let said = crate::land::requeue::requeue(queue_dir.path(), "lane", "the repeated error was fixed").unwrap();

    assert!(said.contains("queued again"), "{said}");
    let store = land_ticket::open(queue_dir.path()).unwrap();
    let instance = land_ticket::instance(queue_dir.path(), OsStr::new(path.file_name().unwrap()));
    assert_eq!(store.current::<LandTicket>(&instance).unwrap().unwrap().state, Phase::Queued);
    assert!(path.is_file() && !quarantined.exists());
    assert!(crate::land::requeue::requeue(queue_dir.path(), "lane", "again").is_err(), "a ticket back in the queue is no longer quarantined");
}

#[test]
fn a_red_fix_is_checked_before_every_urgent_ticket_queued_ahead_of_it() {
    let queue_dir = tempfile::tempdir().unwrap();
    for (micros, branch, urgent, red_fix) in [(1_000, "n882-observe", true, false), (2_000, "n909-policy", true, false), (3_000, "yq-error-forms", false, true)] {
        let path = queue_dir.path().join(queue::file_name(micros, branch));
        let ticket = Ticket { branch: branch.to_owned(), queued_micros: Some(micros), urgent: urgent.then(|| "lander work".to_owned()), red_fix: red_fix.then_some(queue::RedFix::Marker), ..Default::default() };
        queue::write(&path, &ticket).unwrap();
        queued(&path, "enqueued").unwrap();
    }
    let order: Vec<String> = queue::collect(queue_dir.path()).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order[0], "yq-error-forms", "on 2026-11-08 the Main-red fixes for GitHub's red Cargo run at bde3db9c68 ranked with 20 urgent tickets queued before them and rode none of the next round's probes: {order:?}");
}
