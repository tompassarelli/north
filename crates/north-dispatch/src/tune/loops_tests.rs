use super::tests::{Issues, ledger};
use super::{Prior, Tick, failing, tick};
use crate::loops::{Checked, Reading, check, parse};
use std::time::{Duration, SystemTime};

const AT: &str = "2026-10-27T06:00:00Z";
const CLOSED_LOOP: &str = "[[loop]]\nname = \"tier\"\ndecision = \"d\"\nsites = [\"a.rs\"]\nsignal = \"s\"\nupdated_by = \"north tune\"\ncadence_hours = 24\nrecord = \"r\"\n";
const OPEN_LOOP: &str = "[[loop]]\nname = \"effort\"\ndecision = \"d\"\nsites = [\"a.rs\"]\nupdated_by = \"by hand\"\ncadence_hours = 24\nowner = \"north#112 Effort is an arm\"\n";

fn first_tick() -> Tick {
    tick(&ledger(), &Prior::default(), &[], &mut Issues::default(), AT, false)
}

fn checked(record_age_hours: u64) -> Checked {
    let updated = SystemTime::now() - Duration::from_secs(record_age_hours * 3_600);
    check(&parse(&format!("{CLOSED_LOOP}{OPEN_LOOP}")).unwrap(), &|_| true, &|_| Reading::At(updated), SystemTime::now())
}

#[test]
fn a_stale_closed_loop_fails_the_tick_and_an_open_loop_with_an_owner_does_not() {
    assert!(!first_tick().failed);
    let fresh = checked(3);
    assert!(fresh.lines.iter().any(|line| line == "effort: open; by hand sets it; owner north#112 Effort is an arm"));
    let open = failing(first_tick(), &Ok(fresh));
    assert!(!open.failed, "{:#?}", open.lines);
    let stale = checked(49);
    assert!(stale.lines.iter().any(|line| line.starts_with("tier: FAIL stale")), "{:#?}", stale.lines);
    assert_eq!(stale.lines.last().unwrap(), "loops: 1 of 2 failing");
    assert!(failing(first_tick(), &Ok(stale)).failed);
}

#[test]
fn an_unreadable_registry_fails_the_tick() {
    assert!(failing(first_tick(), &Err("loops registry: bad".into())).failed);
}
