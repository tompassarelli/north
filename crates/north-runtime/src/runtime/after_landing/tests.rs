use super::super::failure::failed;
use super::super::follow::lander_cleared;
use super::super::layout::{self, Layout};
use super::super::order::Order;
use super::super::review_hold::{Holds, Screen, now};
use super::super::update::slot::held_by_review;
use super::super::walk::ship_if_walked;
use super::{Landing, Ledger, run_of};
use std::cell::Cell;
use std::fs;
use std::os::unix::fs::symlink;

const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";
const TIP: &str = "3333333333333333333333333333333333333333";
const RERUN: &str = "4444444444444444444444444444444444444444";
const UNMARKED: &str = "5555555555555555555555555555555555555555";
const STALE_RED: &str = "6666666666666666666666666666666666666666";
const BATCHED: &str = "7777777777777777777777777777777777777777";
const ORIGIN: &str = "git@github.com:tompassarelli/north.git";

fn row(head: &str, class: &str, covered_by: &str, fresh: &str) -> String {
    format!("1794550000\tnorth\t{head}\t{class}\tdetail of {class}\tlane-a\t300\t{covered_by}\t120\t\t{fresh}\n")
}

#[test]
fn the_lander_s_post_land_row_and_runner_record_decide_a_head_s_after_landing_verdict() {
    let home = tempfile::tempdir().unwrap();
    let ledger = Ledger::of(home.path(), ORIGIN);
    assert!(matches!(ledger.landing(NEW, 100), Landing::Missing(why) if why.contains("post-land-222222222222") && why.contains("no after-landing ledger")), "a host that keeps no ledger refuses every head");
    fs::create_dir_all(ledger.rows.parent().unwrap()).unwrap();
    let rows = [row(NEW, "red-main", "", "fresh"), row(NEW, "green", TIP, "fresh"), row(TIP, "green", "", "fresh"), row(OLD, "green", TIP, "fresh"), row(RERUN, "red-main", "", "fresh"), row(RERUN, "green", "", "fresh"), row(UNMARKED, "green", "", ""), row(STALE_RED, "red-main", "", ""), row(STALE_RED, "green", TIP, "fresh"), row(OLD, "red-main", "", "fresh").replace("\tnorth\t", "\tsouth\t")];
    fs::write(&ledger.rows, rows.concat()).unwrap();
    assert_eq!(ledger.landing(NEW, 100), Landing::Failed { run: run_of(NEW), class: "red-main".to_owned(), detail: "detail of red-main".to_owned() }, "a head's own failed run decides even when a later tip's green run covers it");
    assert_eq!(ledger.landing(OLD, 100), Landing::Green, "a head with no run of its own takes the green of the batch tip that covered it, and another repository's rows are not its own");
    assert_eq!(ledger.landing(TIP, 100), Landing::Green);
    assert_eq!(ledger.landing(RERUN, 100), Landing::Green, "the last run of a head decides");
    assert!(matches!(ledger.landing(UNMARKED, 100), Landing::Missing(_)), "an own green that may have reused verdicts is no verdict, and no covering run stands in for it");
    assert!(matches!(ledger.landing(STALE_RED, 100), Landing::Failed { ref class, .. } if class == "red-main"), "an own red counts whatever its mark, so a later tip's fresh green does not pass the head");
    fs::create_dir_all(&ledger.logs).unwrap();
    fs::write(ledger.logs.join(format!("{}.runner", run_of(TIP))), format!("{{\"group\":4242,\"started\":60.5,\"deadline\":900,\"base\":\"{OLD}\",\"head\":\"{TIP}\",\"heads\":[\"{BATCHED}\",\"{TIP}\"],\"host\":null}}")).unwrap();
    assert_eq!(ledger.landing(BATCHED, 100), Landing::Running { run: run_of(TIP), group: 4242, age: 40 }, "a head in a batch whose run is recorded as running is running, whether or not its process lives");
    assert!(matches!(ledger.landing(OLD, 100), Landing::Green));
    let refusal = Landing::Running { run: run_of(TIP), group: 4242, age: 40 }.refusal().unwrap();
    assert!(refusal.contains("post-land-333333333333") && refusal.contains("4242") && refusal.contains("40 s"), "{refusal}");
}

#[test]
fn every_promotion_path_refuses_a_head_whose_after_landing_run_is_running_failed_or_missing_naming_the_run_and_takes_it_once_green() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    for sha in [OLD, NEW] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(OLD), layout.current()).unwrap();
    let screen_of = |landing: Landing| Screen { holds: Holds(state.path().join("holds")), contains: Box::new(|_, _| Ok(false)), landed: Box::new(|_| Ok(Vec::new())), after_landing: Box::new(move |_| landing.clone()) };
    let shipped = Cell::new(0);
    let ship = |_: &str| {
        shipped.set(shipped.get() + 1);
        Ok(())
    };
    let not_green = [Landing::Running { run: run_of(NEW), group: 4242, age: 30 }, Landing::Failed { run: run_of(NEW), class: "red-main".to_owned(), detail: "a test failed".to_owned() }, Landing::Missing(format!("{} never ran", run_of(NEW)))];
    for landing in &not_green {
        let screen = screen_of(landing.clone());
        let named = |refused: &str| refused.contains("post-land-222222222222") && refused.contains(&landing.refusal().unwrap());
        let ordered = Order::cleared(NEW.to_owned(), "a reviewed fix".to_owned(), "f-flow-lead".to_owned(), |sha| screen.order(sha, &["whiterabbit"], now())).err().map(|failure| failure.message).unwrap_or_default();
        assert!(named(&ordered), "north runtime order took a head whose after-landing run had not ended green; the rule was kept by hand through a held promotion lock: {ordered:?}");
        let promoted = held_by_review(&layout, &screen, NEW).unwrap_or_default();
        assert!(named(&promoted), "the slot and Live-fix promotion after a landing refuses it: {promoted:?}");
        assert_eq!(ship_if_walked(Ok(NEW.to_owned()), &screen, ship).unwrap(), None, "the walk ships it to no other host");
        assert_eq!(shipped.get(), 0);
        fs::remove_file(layout.current()).unwrap();
        symlink(layout::link(NEW), layout.current()).unwrap();
        let linked = lander_cleared(&layout, &screen).err().map(|failure| failure.message).unwrap_or_default();
        assert!(named(&linked), "the lander link stays off it: {linked:?}");
        fs::remove_file(layout.current()).unwrap();
        symlink(layout::link(OLD), layout.current()).unwrap();
    }
    let screen = screen_of(Landing::Green);
    let order = Order::cleared(NEW.to_owned(), "a reviewed fix".to_owned(), "f-flow-lead".to_owned(), |sha| screen.order(sha, &["whiterabbit"], now())).unwrap();
    assert_eq!(order.sha, NEW);
    assert!(held_by_review(&layout, &screen, NEW).is_none());
    assert_eq!(ship_if_walked(Err("rolled back".to_owned()), &screen, ship).unwrap(), None, "nothing ships until the runtime walked the spine here");
    assert_eq!(shipped.get(), 0);
    assert_eq!(ship_if_walked(Ok(NEW.to_owned()), &screen, ship).unwrap(), Some(()));
    assert_eq!(shipped.get(), 1);
    assert!(ship_if_walked(Ok(NEW.to_owned()), &screen, |_| Err::<(), _>(failed(1, "unreachable"))).is_err());
    fs::remove_file(layout.current()).unwrap();
    symlink(layout::link(NEW), layout.current()).unwrap();
    assert!(lander_cleared(&layout, &screen).is_ok());
}
