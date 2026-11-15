use super::super::layout::{self, Layout};
use north_runtime_records::after_landing::Landing;
use super::super::main_runs::{Kept, MainRun, MainRuns};
use super::super::review_hold::Screen;
use north_runtime_records::hold_ledger::{Ask, Caller, Holds, now};
use super::super::trip::Trips;
use super::{held_out, ship_if_walked};
use north_base::platform::host;
use north_offload::land_offload::Deployment;
use std::fs;

const PINNED: &str = "1111111111111111111111111111111111111111";
const MAIN: &str = "2222222222222222222222222222222222222222";

fn screen_holding(holds: &std::path::Path) -> Screen {
    Screen { holds: Holds(holds.to_path_buf()), contains: Box::new(|_, _| Ok(true)), landed: Box::new(|_| Ok(Vec::new())), answered: Box::new(|_, _| false), after_landing: Some(Box::new(|_| Landing::Green)), runs: MainRuns::none(), trips: Trips(holds.join("trips")), revert: Box::new(|_| Ok(())), replay: crate::runtime::replay::regressing() }
}

#[test]
fn a_hold_naming_one_host_leaves_only_that_host_out_of_the_ship_and_a_hold_naming_none_refuses_all() {
    let caller = Caller { session: "f-review".to_owned(), coordinator: false, reviewer: true };
    let here = host::name();
    let plan = Deployment { names: vec![here.clone(), "elsewhere".to_owned()], destinations: vec!["ssh-here".to_owned(), "ssh-elsewhere".to_owned()], ..Default::default() };

    let checkout = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| std::process::Command::new("git").current_dir(checkout.path()).args(arguments).output().unwrap();
    git(&["init", "-q"]);
    git(&["-c", "user.name=north", "-c", "user.email=north@invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "reviewed"]);
    let head = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout).unwrap();
    let held = head.trim();

    let scoped = tempfile::tempdir().unwrap();
    let screen = screen_holding(scoped.path());
    let place = |host: Option<&str>| Ask::Place { commit: held.to_owned(), lane: "f-fix".to_owned(), host: host.map(str::to_owned), until: None, why: "the elsewhere canary reads the wrong lock".to_owned() };
    screen.holds.serve(place(Some("elsewhere")), &caller, now()).unwrap();
    assert!(screen.order(MAIN, &[here.as_str()], now()).is_ok(), "a hold naming another host refused this host's own ship");
    let (kept, left_out) = held_out(&plan, &screen, MAIN, now());
    assert_eq!(kept, vec!["ssh-here".to_owned()], "the held host was not the only one left out");
    assert_eq!(left_out.len(), 1);
    assert_eq!(left_out[0].0, "ssh-elsewhere");
    assert!(left_out[0].1.contains(held), "the left-out host is not named with its hold: {:?}", left_out[0].1);

    let unscoped = tempfile::tempdir().unwrap();
    let screen = screen_holding(unscoped.path());
    screen.holds.serve(place(None), &caller, now()).unwrap();
    assert!(screen.order(MAIN, &[here.as_str()], now()).is_err(), "a hold naming no host let this host ship");
    assert!(screen.order(MAIN, &["elsewhere"], now()).is_err(), "a hold naming no host let another host ship");
    let (kept, left_out) = held_out(&plan, &screen, MAIN, now());
    assert!(kept.is_empty());
    assert_eq!(left_out.len(), 2);
}

#[test]
fn a_shipped_runtime_is_swapped_in_on_every_unheld_host_when_a_red_run_on_its_ancestry_ends_after_it_went_live() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    fs::create_dir_all(layout.runtime(MAIN)).unwrap();
    std::os::unix::fs::symlink(layout::link(MAIN), layout.current()).unwrap();
    let red_ancestor = MainRuns { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: PINNED.to_owned(), id: "43638604894".to_owned(), red: true }]))), kept: Box::new(|_| Ok(Kept::Log("test\tTest\t2026-11-14T20:55:41.0Z [2026-11-14T20:55:41.000+00:00]         FAIL [   0.526s] north::it cli::parses".to_owned()))), alert: Box::new(|_| {}) };
    let screen = Screen { after_landing: None, runs: red_ancestor, trips: Trips(state.path().join("trips")), ..screen_holding(&state.path().join("holds")) };
    assert!(screen.order(PINNED, &[host::name().as_str()], now()).is_err() && screen.order(MAIN, &[host::name().as_str()], now()).is_ok(), "the red run of the ancestor trips the ancestor alone, and the later head goes live and is verified after (north#1537)");
    let shipped = std::cell::Cell::new(false);
    let ship = |sha: &str, _: &[String]| {
        shipped.set(sha == MAIN);
        Ok(())
    };
    assert_eq!(ship_if_walked(&layout, Ok(MAIN.to_owned()), &screen, ship).unwrap(), Some(()), "on 2026-11-15 whiterabbit took 4c4d4309 live, the hosted run of its ancestor c00b4ec9 ended red before its walk, and the walk shipped it to no other host, so gex45 and cpx62b kept 2091ad27 with 4c4d4309 unpacked beside it");
    assert!(shipped.get());
    assert!(super::super::follow::lander_cleared(&layout, &screen).is_ok(), "the lander link follows the runtime live here");
}
