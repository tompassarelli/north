use super::super::canary::{self, Machine, Ran, Rollback};
use north_runtime_records::failure::Failure;
use super::super::layout::{self, Layout};
use super::super::lifecycle::{self, Canarying};
use north_runtime_records::after_landing::Landing;
use super::super::review_hold::{Screen, answered_in, landed_in};
use north_runtime_records::hold_ledger::{self, Ask, Caller, Holds, lane_of, now};
use super::super::main_runs::{Kept, MainRun, MainRuns};
use super::super::trip::Trips;
use super::super::update;
use super::{Host, Order, run};
use std::fs;
use std::os::unix::fs::symlink;
use north_base::platform::review_accept::{Accept, Accepts, patch_id};
use north_trusted_mirror::Mirror;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";
const REVIEWED: &str = "4444444444444444444444444444444444444444";
const SQUASHED: &str = "5555555555555555555555555555555555555555";

struct Probes {
    failing: Option<&'static str>,
    retired: Option<&'static str>,
}

impl Machine for Probes {
    fn listed_by(&self, host: &str, sha: &str) -> Result<canary::CoreLoop, String> {
        let mut list = canary::compiled(host, sha, self);
        if sha == NEW {
            list.probes.retain(|probe| !self.retired.is_some_and(|retired| probe.name.contains(retired)));
        }
        Ok(list)
    }

    fn probe(&self, probe: &canary::Probe) -> Ran {
        match self.failing.is_some_and(|failing| probe.name.contains(failing)) {
            true => Ran::Exited { code: 1, log: "the probe failed".to_owned() },
            false => Ran::Exited { code: 0, log: String::new() },
        }
    }

    fn roll_back(&self, _sha: &str) -> Rollback {
        panic!("a runtime that is not live is never rolled back")
    }

    fn alarm(&self, text: &str) {
        panic!("an ordered promotion alarms nothing before it is live: {text}")
    }

    fn page(&self, key: &str, _text: &str) {
        panic!("an ordered promotion pages nothing before it is live: {key}")
    }

    fn rollback_target(&self, _sha: &str) -> Option<String> {
        None
    }

    fn launchable_providers(&self) -> Vec<String> {
        vec!["claude".to_owned()]
    }

    fn sessions_reach_hub(&self, _host: &str) -> bool {
        true
    }

    fn offloads_to_hub(&self, _host: &str) -> bool {
        true
    }

    fn settles_fleet(&self, _host: &str) -> bool {
        false
    }

    fn takes_land_checks(&self, _host: &str) -> bool {
        false
    }

    fn lander_repositories(&self) -> Option<canary::Landers> {
        None
    }
}

struct Rehearsal<'a> {
    layout: &'a Layout,
    name: &'static str,
    failing: Option<&'static str>,
    retired: Option<&'static str>,
    live: Mutex<String>,
    pin: Mutex<Option<String>>,
    steps: &'a Mutex<Vec<String>>,
    stays: bool,
    waits: Option<&'static str>,
}

impl Rehearsal<'_> {
    fn step(&self, what: &str) {
        self.steps.lock().unwrap().push(format!("{} {what}", self.name));
    }
}

impl Host for Rehearsal<'_> {
    fn name(&self) -> &str {
        self.name
    }

    fn pinned(&self) -> Result<bool, Failure> {
        Ok(self.pin.lock().unwrap().is_some())
    }

    fn acquire(&self, _sha: &str) -> Result<(), Failure> {
        self.step("staged");
        Ok(())
    }

    fn canary(&self, sha: &str) -> Result<(), String> {
        self.step("canary");
        if let Some(other) = self.waits {
            let staged = format!("{other} staged");
            assert!(north_base::platform::wait::until(std::time::Duration::from_secs(3600), || self.steps.lock().unwrap().contains(&staged)), "{} judged {} before {other} was shipped it", self.name, layout::short(sha));
        }
        canary::preflight(self.layout, self.name, sha, self.live().as_deref(), &Probes { failing: self.failing, retired: self.retired })
    }

    fn go_live(&self, sha: &str) -> Result<(), Failure> {
        self.step("live");
        if !self.stays {
            *self.live.lock().unwrap() = sha.to_owned();
        }
        Ok(())
    }

    fn live(&self) -> Option<String> {
        Some(self.live.lock().unwrap().clone())
    }

    fn write_pin(&self, sha: &str) -> Result<(), Failure> {
        *self.pin.lock().unwrap() = Some(sha.to_owned());
        Ok(())
    }
}

#[test]
fn an_order_promotes_a_candidate_failing_only_a_probe_it_retired_and_refuses_one_failing_a_probe_it_kept() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    for sha in [OLD, NEW] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(OLD), layout.current()).unwrap();
    let steps = Mutex::new(Vec::new());
    let host = |name, failing| Rehearsal { layout: &layout, name, failing, retired: Some("depbox"), live: Mutex::new(OLD.to_owned()), pin: Mutex::new(None), steps: &steps, stays: false, waits: None };
    let (settling, builder) = (host("whiterabbit", Some("depbox")), host("gex45", Some("ssh reaches no host")));
    let order = Order { sha: NEW.to_owned(), reason: "the depbox probe is retired".to_owned(), orderer: "f-canary-chain".to_owned() };

    let refused = run(&layout, &order, &settling, &[&builder], &[], || Ok(()), |()| Ok(false)).unwrap_err();

    assert_eq!(settling.live.lock().unwrap().as_str(), NEW, "the order's own list still held the depbox probe, which the candidate retired and would fail");
    let judged = lifecycle::record_of::<Canarying>(&layout, &format!("whiterabbit/{NEW}")).unwrap().unwrap().cause;
    assert!(judged.contains("probes added: none; probes removed: `a worker command's depbox run execs the judged north`"), "{judged}");
    assert_eq!(builder.live.lock().unwrap().as_str(), OLD);
    assert!(refused.message.contains("gex45 stays on its old runtime") && refused.message.contains("ssh reaches no host"), "{}", refused.message);
}

fn squashed(commit: &str, head: &str) -> Result<bool, String> {
    Ok(commit == head || (commit == SQUASHED && head == NEW))
}

fn newer_green(commit: &str, head: &str) -> Result<bool, String> {
    Ok(head == NEW && (commit == SQUASHED || commit == NEW))
}

fn landings(lane: &str) -> Result<Vec<String>, String> {
    Ok(if lane == "f-fix" { vec![SQUASHED.to_owned()] } else { Vec::new() })
}

const NO_TRIPS: &str = "/nonexistent/north-test-trips";
const HOSTED_LOG: &str = "test\tTest\t2026-11-13T19:24:41.7710459Z [2026-11-13T19:24:41.767+00:00]         FAIL [   0.526s] north::it golden_path::a_brief_becomes_a_worker_commit_that_the_lander_lands_on_main_and_the_records_and_lane_show_it_after_a_stop_that_outwaits_a_slow_client";
const TIMED_OUT_LOG: &str = "test\tTest\t2026-11-14T14:34:01.4024061Z [2026-11-14T14:34:01.347+00:00]      TIMEOUT [ 122.659s] north-landing land::queue::tests::two_threads_finishing_one_ticket_leave_it_whole\ntest\tTest\t2026-11-14T14:43:02.4961561Z [2026-11-14T14:43:02.494+00:00] ✉ Timeout: root//crates/north-lander:north-lander-test-it (10:00.0s)";

fn screen_of(holds: Holds, contains: fn(&str, &str) -> Result<bool, String>, landed: fn(&str) -> Result<Vec<String>, String>) -> Screen {
    Screen { holds, contains: Box::new(move |commit, head| Ok(commit == crate::runtime::main_runs::FIXTURE_ANCESTOR || contains(commit, head)?)), landed: Box::new(landed), answered: Box::new(|_, _| false), after_landing: Some(Box::new(|_| Landing::Green)), trips: Trips(PathBuf::from(NO_TRIPS)), runs: MainRuns::none(), revert: Box::new(|_| Ok(())), replay: crate::runtime::replay::regressing() }
}

#[test]
fn holds_and_trips_refuse_a_head_until_lifted_or_cleared_and_a_trip_naming_its_first_failing_commit_reverts_it() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    for sha in [OLD, NEW] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(OLD), layout.current()).unwrap();
    let steps = Mutex::new(Vec::new());
    let settling = Rehearsal { layout: &layout, name: "whiterabbit", failing: None, retired: None, live: Mutex::new(OLD.to_owned()), pin: Mutex::new(None), steps: &steps, stays: false, waits: None };
    let checkout = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| std::process::Command::new("git").current_dir(checkout.path()).args(arguments).output().unwrap();
    git(&["init", "-q"]);
    git(&["-c", "user.name=north", "-c", "user.email=north@invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "reviewed"]);
    let head = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout).unwrap();
    let held = head.trim();
    let store = state.path().join("holds");
    let screen = screen_of(Holds(store.clone()), squashed, landings);
    let cleared = |sha: &str| screen.order(sha, &["whiterabbit"], now());
    let ordered = || Order::cleared(NEW.to_owned(), "a reviewed pipeline fix".to_owned(), "f-flow-lead".to_owned(), cleared);
    let session = |name: &str| Caller { session: name.to_owned(), coordinator: false, reviewer: false };
    let (reviewer, author) = (session("f-review"), session("f-holds"));
    let place = |caller: &Caller| screen.holds.serve(Ask::Place { commit: held.to_owned(), lane: "f-fix".to_owned(), host: None, until: None, why: "the canary reads the wrong lock".to_owned() }, caller, now());
    let lift = |caller: &Caller| screen.holds.serve(Ask::Lift { commit: held.to_owned(), why: "the fix is reviewed".to_owned() }, caller, now());
    place(&reviewer).unwrap();
    let environment = [("NORTH_VERDICT_CACHE_DIR".to_owned(), state.path().display().to_string())];
    let listed = hold_ledger::standing(&environment, state.path()).unwrap();
    assert!(matches!(listed.as_slice(), [entry] if entry.commit == held && entry.session == "f-review"), "`north holds` lists a placed hold with its commit and holder: {listed:?}");

    let refused = ordered().err().map(|failure| failure.message).unwrap_or_default();
    assert!(refused.contains(held) && refused.contains("f-review"), "a reviewer's hold was a message to the coordinator, who ordered each runtime by hand, and the lander lands the held lane as a squashed commit of its own: {refused:?}");
    let promotion = update::slot::held_by_review(&layout, &screen, NEW).unwrap_or_default();
    assert!(promotion.contains(held) && promotion.contains("f-review"), "the slot and Live-fix promotion refuses the same head: {promotion:?}");
    for (settler, case) in [(Ok(true), "another host settles the fleet"), (Err("no such file".to_owned()), "its orchestration configuration cannot be read")] {
        assert!(update::slot::screened(&layout, state.path(), NEW.to_owned(), false, settler, &screen).is_none(), "a host that follows origin/main itself keeps no holds and no push ledger, so it takes no head of its own when {case}");
    }

    let not_theirs = lift(&author).unwrap_err();
    assert!(not_theirs.contains(held) && not_theirs.contains("f-review"), "{not_theirs}");
    assert!(ordered().is_err(), "a lift by another session leaves the hold standing");

    lift(&reviewer).unwrap();
    assert!(hold_ledger::standing(&environment, state.path()).unwrap().is_empty(), "and stops listing it once it is lifted");
    run(&layout, &ordered().unwrap(), &settling, &[], &[], || Ok(()), |()| Ok(false)).unwrap();
    assert_eq!(settling.live.lock().unwrap().as_str(), NEW);
    assert!(update::slot::held_by_review(&layout, &screen, NEW).is_none());

    place(&reviewer).unwrap();
    lift(&reviewer).unwrap();
    let named = cleared(NEW).unwrap();
    assert!(named.iter().any(|line| line.contains("f-review") && line.contains("f-review lifted it: the fix is reviewed")), "{named:?}");
    let unresolved = screen_of(Holds(store.clone()), |_, _| Err("fatal: Not a valid object name".to_owned()), landings);
    assert!(unresolved.order(NEW, &["whiterabbit"], now()).is_ok(), "on 2026-11-13 a lifted hold on a commit the checkout lacks refused every order and promotion for a day");
    place(&reviewer).unwrap();

    let (unlisted, unparsed, misnamed) = (state.path().join("not-a-store"), state.path().join("unparsed"), state.path().join("misnamed"));
    fs::write(&unlisted, "").unwrap();
    for store in [&unparsed, &misnamed] {
        fs::create_dir_all(store).unwrap();
    }
    fs::write(unparsed.join("broken.json"), "{").unwrap();
    fs::copy(store.join(format!("{held}.f-review.json")), misnamed.join(format!("{NEW}.f-review.json"))).unwrap();
    let failing: [(&str, Screen, &str); 5] = [
        ("a store that cannot be listed", screen_of(Holds(unlisted), squashed, landings), "not-a-store"),
        ("an entry that is no hold", screen_of(Holds(unparsed), squashed, landings), "broken.json"),
        ("an entry not named for its hold", screen_of(Holds(misnamed), squashed, landings), "not the hold its name says"),
        ("git failing", screen_of(Holds(store.clone()), |_, _| Err("fatal: no such object".to_owned()), landings), "no such object"),
        ("a push ledger that does not exist", Screen { holds: Holds(store), contains: Box::new(|commit, _| Ok(commit == crate::runtime::main_runs::FIXTURE_ANCESTOR)), landed: landed_in(state.path().join("pushes.tsv")), answered: Box::new(|_, _| false), after_landing: Some(Box::new(|_| Landing::Green)), trips: Trips(PathBuf::from(NO_TRIPS)), runs: MainRuns::none(), revert: Box::new(|_| Ok(())), replay: crate::runtime::replay::regressing() }, "pushes.tsv"),
    ];
    for (case, screen, says) in failing {
        let refused = screen.order(NEW, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
        assert!(refused.contains("does not go live") && refused.contains(says), "{case}: {refused}");
    }
    let trips = state.path().join("trips");
    fs::create_dir_all(&trips).unwrap();
    fs::write(trips.join(format!("{SQUASHED}.trip")), "north\tcli::parses\n").unwrap();
    let tripped = Screen { trips: Trips(trips.clone()), after_landing: None, ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(tripped.order(NEW, &["whiterabbit"], now()).is_ok(), "on 2026-11-15 the trips on ancestors held every later head from 3:55 AM, the fixes for the live runtime among them: a later head with no after-landing run of its own goes live and is verified after (north#1537)");
    let refused = tripped.order(SQUASHED, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
    assert!(refused.contains(SQUASHED) && refused.contains("cli::parses") && refused.contains("north runtime trip clear"), "an open Buck2 trip refuses the order of its own commit and names the trip: {refused}");
    let passing = Screen { trips: Trips(trips.clone()), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(passing.order(NEW, &["whiterabbit"], now()).is_ok(), "on 2026-11-14 a Buck2 trip on an ancestor held every later head for an hour until cleared by hand, though the head's own after-landing run passed");
    assert!(passing.order(SQUASHED, &["whiterabbit"], now()).is_err(), "a trip still holds its own commit");
    let promoted = tripped.order(SQUASHED, &["whiterabbit"], now()).err().unwrap_or_default();
    assert!(promoted.contains(SQUASHED), "the slot and Live-fix promotion refuses the same head: {promoted}");
    assert!(tripped.order(OLD, &["whiterabbit"], now()).is_ok(), "a head before the tripped head is not refused");
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let recorder = |asked: &std::rc::Rc<std::cell::RefCell<Vec<String>>>| -> crate::runtime::review_hold::Revert {
        let seen = asked.clone();
        Box::new(move |trip| {
            seen.borrow_mut().push(trip.revert_request("parent", "tree"));
            Ok(())
        })
    };
    let reverting = Screen { trips: Trips(trips.clone()), revert: recorder(&asked), after_landing: None, ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    let tripped_at = now();
    let later = tripped_at + crate::runtime::trip::REVERT_AFTER_SECONDS + 1;
    let stamp = |record: std::path::PathBuf| fs::File::options().write(true).open(record).unwrap().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(tripped_at)).unwrap();
    stamp(trips.join(format!("{SQUASHED}.trip")));
    assert!(reverting.order(NEW, &["whiterabbit"], tripped_at).is_ok() && asked.borrow().is_empty(), "a trip younger than five minutes asks for no revert");
    assert!(reverting.order(NEW, &["whiterabbit"], later).is_ok() && reverting.order(NEW, &["whiterabbit"], later).is_ok());
    let requests = asked.borrow().clone();
    assert_eq!(requests.len(), 1, "since 315767a4c7 lanes land before their build-host tests, and on 2026-11-14 the trips of four culprits held every promotion: a trip that stood five minutes asks once for the revert of its commit");
    assert!(requests[0].starts_with(&format!("Main red at {SQUASHED}")) && requests[0].contains("test cli::parses ... FAILED") && requests[0].contains("\nLive-fix: "), "{}", requests[0]);
    Trips(trips.clone()).clear(SQUASHED, "coordinator", "the case passes on rerun", now()).unwrap();
    assert!(tripped.order(SQUASHED, &["whiterabbit"], now()).is_ok(), "a cleared trip no longer holds");
    assert!(Trips(trips).clear(SQUASHED, "coordinator", "again", now()).is_err(), "a trip is cleared once");
    let hosted = state.path().join("hosted-trips");
    let reds_only = || MainRuns { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: SQUASHED.to_owned(), id: "43491019011".to_owned(), red: true }]))), kept: Box::new(|_| Ok(Kept::Log(HOSTED_LOG.to_owned()))), alert: Box::new(|_| {}) };
    let passing = Screen { runs: reds_only(), trips: Trips(hosted.clone()), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(passing.order(NEW, &["whiterabbit"], now()).is_ok(), "a red hosted run on an ancestor holds no later head whose own after-landing run passed");
    let read = Screen { runs: reds_only(), trips: Trips(hosted.clone()), after_landing: None, ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(read.order(NEW, &["whiterabbit"], now()).is_ok(), "a red hosted run on an ancestor holds no later head, which is verified after");
    let refused = read.order(SQUASHED, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
    assert!(refused.contains(SQUASHED) && refused.contains("golden_path::a_brief_becomes") && refused.contains("north runtime trip clear"), "a newest red hosted run of main whose case fails again records its trip and refuses its own head with its case: {refused}");
    assert_eq!(fs::read_to_string(hosted.join(format!("{SQUASHED}.trip"))).unwrap(), "run\t43491019011\nnorth\tgolden_path::a_brief_becomes_a_worker_commit_that_the_lander_lands_on_main_and_the_records_and_lane_show_it_after_a_stop_that_outwaits_a_slow_client\n");
    Trips(hosted).clear(SQUASHED, "coordinator", "the case passes on rerun", now()).unwrap();
    assert!(read.order(SQUASHED, &["whiterabbit"], now()).is_ok(), "a cleared hosted trip no longer holds");
    let timed_out = state.path().join("timed-out-trips");
    for (run, kept) in [("43604836319", Kept::Log(TIMED_OUT_LOG.to_owned())), ("43607761294", Kept::CutOff("The hosted runner lost communication with the server.".to_owned()))] {
        let runs = MainRuns { page: Box::new(move |number| Ok((number == 1).then(|| vec![MainRun { head: SQUASHED.to_owned(), id: run.to_owned(), red: true }, MainRun { head: crate::runtime::main_runs::FIXTURE_ANCESTOR.to_owned(), id: "0".to_owned(), red: false }]))), kept: Box::new(move |_| Ok(kept.clone())), alert: Box::new(|_| {}) };
        let unjudged = Screen { runs, trips: Trips(timed_out.join(run)), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
        assert!(unjudged.order(NEW, &["whiterabbit"], now()).is_ok(), "on 2026-11-14 hosted run {run} only timed out or hit its job limit, tripped promotion, and was cleared by hand: such a run trips nothing");
        assert!(Trips(timed_out.join(run)).read(SQUASHED).unwrap().is_none() && Trips(timed_out.join(run)).unjudged(SQUASHED).unwrap(), "{run} is recorded as not judged, not as a trip");
    }
    let superseded_trips = state.path().join("superseded-trips");
    let superseded_runs = || MainRuns { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: NEW.to_owned(), id: "43493048821".to_owned(), red: false }, MainRun { head: SQUASHED.to_owned(), id: "43491019011".to_owned(), red: true }]))), kept: Box::new(|_| Ok(Kept::Log(HOSTED_LOG.to_owned()))), alert: Box::new(|_| {}) };
    let superseded = superseded_runs();
    let newer = Screen { contains: Box::new(newer_green), runs: superseded, trips: Trips(superseded_trips.clone()), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(newer.order(NEW, &["whiterabbit"], now()).is_ok(), "an older red run behind a newer green run on the ancestry holds nothing");
    assert!(Trips(superseded_trips).read(SQUASHED).unwrap().is_none(), "an older red behind a newer green run writes no trip");
    let fixed_trips = state.path().join("fixed-trips");
    fs::create_dir_all(&fixed_trips).unwrap();
    fs::write(fixed_trips.join(format!("{SQUASHED}.trip")), "run\t43491019011\nnorth\tcli::parses\n").unwrap();
    stamp(fixed_trips.join(format!("{SQUASHED}.trip")));
    let fixed_asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let fixed = Screen { contains: Box::new(newer_green), runs: superseded_runs(), trips: Trips(fixed_trips), revert: recorder(&fixed_asked), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(fixed.order(NEW, &["whiterabbit"], later).is_ok() && fixed_asked.borrow().is_empty(), "a fix that landed first and turned main green asks for no revert");
    let off_ancestry = MainRuns { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: OLD.to_owned(), id: "43491019011".to_owned(), red: true }]))), kept: Box::new(|_| Ok(Kept::Log(HOSTED_LOG.to_owned()))), alert: Box::new(|_| {}) };
    let off = Screen { runs: off_ancestry, trips: Trips(state.path().join("off-ancestry-trips")), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    let unscreened = off.order(NEW, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
    assert!(unscreened.contains("is not screened"), "a list whose runs are all off the head's ancestry refuses the head as not screened: {unscreened}");
    let ledger = format!("1794548552\tnorth\tf-fix\t{REVIEWED}\t{SQUASHED}\tf-fix\n");
    assert_eq!(lane_of(REVIEWED, &ledger, &[]), Ok("f-fix".to_owned()));
    for branches in [vec!["main".to_owned()], vec!["f-fix".to_owned(), "f-other".to_owned()]] {
        assert!(lane_of(REVIEWED, "", &branches).is_err(), "a hold whose lane is not one branch is refused at placement: {branches:?}");
    }
    super::trusted_store_tests::a_promotion_an_order_and_a_review_screen_refuse_what_a_worker_writes_into_the_main_store();
}

#[test]
fn a_red_run_that_cannot_be_read_refuses_the_head_and_records_nothing_until_a_read_judges_it() {
    let state = tempfile::tempdir().unwrap();
    let unread_trips = state.path().join("unread-trips");
    Trips(unread_trips.clone()).write_unjudged(SQUASHED, "43491019011", "its log cannot be read, as when the job ends at its time limit").unwrap();
    let counted = std::rc::Rc::new(std::cell::Cell::new(0));
    let flaky = MainRuns { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: SQUASHED.to_owned(), id: "43491019011".to_owned(), red: true }, MainRun { head: crate::runtime::main_runs::FIXTURE_ANCESTOR.to_owned(), id: "0".to_owned(), red: false }]))), kept: Box::new(move |_| { counted.set(counted.get() + 1); if counted.get() == 1 { Err("gh: connection refused".to_owned()) } else { Ok(Kept::Log(HOSTED_LOG.to_owned())) } }), alert: Box::new(|_| {}) };
    let unread = Screen { runs: flaky, trips: Trips(unread_trips.clone()), after_landing: None, ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    let unread_refusal = unread.order(SQUASHED, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
    assert!(unread_refusal.contains("is not screened") && !Trips(unread_trips.clone()).unjudged(SQUASHED).unwrap(), "a review on 2026-11-15 found that a failed read of a red run marked it not judged for good, so a red run could promote: such a record counts as absent, and a failed read refuses the head and records nothing: {unread_refusal}");
    let reread = unread.order(SQUASHED, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
    assert!(reread.contains("golden_path::a_brief_becomes") && Trips(unread_trips).read(SQUASHED).unwrap().is_some(), "the next screen reads the run again and trips on its failed case: {reread}");
}

const PLACER: &str = "f-review";

fn git_in(checkout: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git").current_dir(checkout).args(["-c", "user.name=north", "-c", "user.email=north@invalid", "-c", "commit.gpgsign=false"]).args(arguments).env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn committed(checkout: &Path, file: &str, text: &str) -> String {
    fs::write(checkout.join(file), text).unwrap();
    git_in(checkout, &["add", file]);
    git_in(checkout, &["commit", "-q", "-m", text]);
    git_in(checkout, &["rev-parse", "HEAD"])
}

fn until_checkout(root: &Path) -> (PathBuf, PathBuf, String) {
    let main = root.join("main");
    fs::create_dir_all(&main).unwrap();
    git_in(&main, &["init", "-q"]);
    committed(&main, "README", "north\n");
    let held = committed(&main, "lock.rs", "wrong lock\n");
    (main, root.join("state"), held)
}

fn accepted(state: &Path, checkout: &Path, commit: &str, session: &str) {
    let patch_id = patch_id(checkout, &format!("{commit}^"), commit).unwrap();
    Accepts(state.join("accepts")).record(&Accept { patch_id, session: session.to_owned(), at: now() }).unwrap();
}

fn until_screen(state: &Path, checkout: &Path, held: &str) -> Screen {
    let screen = Screen { holds: Holds(state.join("holds")), contains: Box::new(|_, _| Ok(true)), landed: landed_in(state.join("pushes.tsv")), answered: answered_in(Mirror::open(checkout).unwrap().git_dir().to_path_buf(), Accepts(state.join("accepts"))), after_landing: Some(Box::new(|_| Landing::Green)), trips: Trips(PathBuf::from(NO_TRIPS)), runs: MainRuns::none(), revert: Box::new(|_| Ok(())), replay: crate::runtime::replay::regressing() };
    let ask = Ask::Place { commit: held.to_owned(), lane: "f-broken".to_owned(), host: None, until: Some("f-cure".to_owned()), why: "the canary reads the wrong lock until f-cure lands".to_owned() };
    screen.holds.serve(ask, &Caller { session: PLACER.to_owned(), coordinator: false, reviewer: true }, now()).unwrap();
    screen
}

fn land(state: &Path, landed: &str) {
    let ledger = state.join("pushes.tsv");
    let rows = fs::read_to_string(&ledger).unwrap_or_default();
    fs::write(&ledger, format!("{rows}1794600000\tnorth\tf-cure\t{OLD}\t{landed}\tf-cure\n")).unwrap();
}

#[test]
fn a_hold_naming_the_lane_whose_landing_lifts_it_lifts_on_its_placers_accepted_landing_read_in_the_trusted_mirror_and_a_trivial_lane_under_the_until_name_does_not_lift_a_review_hold() {
    let root = tempfile::tempdir().unwrap();
    let (main, state, held) = until_checkout(root.path());
    let trivial = committed(&main, "README", "north, cured\n");
    let unaccepted = committed(&main, "lock.rs", "another lock\n");
    let cure = committed(&main, "lock.rs", "right lock\n");
    let forged = committed(&main, "lock.rs", "a worker's lock\n");
    accepted(&state, &main, &trivial, PLACER);
    accepted(&state, &main, &unaccepted, "f-other-review");
    accepted(&state, &main, &cure, PLACER);
    git_in(&main, &["replace", &forged, &cure]);
    assert_eq!(patch_id(&main, &format!("{forged}^"), &forged), patch_id(&main, &format!("{cure}^"), &cure), "the planted replace object does not make the checkout answer the accepted cure's patch for the worker's landing");
    Mirror::open(&main).unwrap();
    git_in(&main, &["switch", "-q", "-c", "f-cure", &cure]);
    let late = committed(&main, "lock.rs", "a later right lock\n");
    accepted(&state, &main, &late, PLACER);
    let screen = until_screen(&state, &main, &held);
    for (landed, why) in [
        ("", "a hold whose cure is queued but not landed"),
        (trivial.as_str(), "a landing under the --until name that changes none of the held commit's files"),
        (unaccepted.as_str(), "a landing under the --until name that edits the held file without its placer's accept"),
        (forged.as_str(), "a worker's landing that a replace object in the checkout passes off as the accepted cure"),
        (late.as_str(), "an accepted landing the trusted mirror lacks"),
    ] {
        land(&state, landed);
        let refused = screen.order(&late, &["whiterabbit"], now()).err().unwrap_or_default();
        assert!(refused.contains(&held) && !refused.contains("not screened"), "{why} lifted the hold or refused the head as unscreened: {refused:?}");
    }

    Mirror::open(&main).unwrap().publish_lane(&main, "f-cure", &late).unwrap();
    let named = screen.order(&late, &["whiterabbit"], now()).unwrap();
    assert!(named.iter().any(|line| line.contains("f-cure landed as") && line.contains(&late)), "on 2026-11-14 a hold stood about an hour after the fix it named had landed, until its placer lifted it by hand: {named:?}");
    let environment = [("NORTH_VERDICT_CACHE_DIR".to_owned(), state.display().to_string())];
    assert!(hold_ledger::standing(&environment, &state).unwrap().is_empty(), "the landed lane leaves no hold standing");
}

#[test]
fn a_red_ancestor_run_is_recorded_paged_and_reverted_while_a_descendant_that_passed_its_own_run_promotes() {
    let state = tempfile::tempdir().unwrap();
    let trips = state.path().join("hosted-trips");
    let paged = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let (pages, asks) = (paged.clone(), asked.clone());
    let runs = MainRuns { page: Box::new(|number| Ok((number == 1).then(|| vec![MainRun { head: SQUASHED.to_owned(), id: "43491019011".to_owned(), red: true }]))), kept: Box::new(|_| Ok(Kept::Log(HOSTED_LOG.to_owned()))), alert: Box::new(move |text| pages.borrow_mut().push(text.to_owned())) };
    let screen = Screen { runs, trips: Trips(trips.clone()), revert: Box::new(move |trip| {
        asks.borrow_mut().push(trip.head.clone());
        Ok(())
    }), ..screen_of(Holds(state.path().join("no-holds")), squashed, landings) };
    assert!(screen.order(NEW, &["whiterabbit"], now()).is_ok(), "a descendant whose own after-landing run passed promotes past a red ancestor");
    assert!(Trips(trips).read(SQUASHED).unwrap().is_some() && paged.borrow().len() == 1, "8e6ec0eaed stopped recording and paging a red hosted run on an ancestor of a passing head");
    assert!(screen.order(NEW, &["whiterabbit"], now() + crate::runtime::trip::REVERT_AFTER_SECONDS + 1).is_ok());
    assert_eq!(asked.borrow().as_slice(), [SQUASHED.to_owned()], "8e6ec0eaed stopped the sheriff revert of a red hosted run on an ancestor of a passing head");
}

#[test]
fn an_order_from_a_confined_seat_runs_this_hosts_canary_in_a_unit_of_the_user_manager_not_in_its_own_process_tree() {
    let root = tempfile::tempdir().unwrap();
    let layout = Layout { state: root.path().to_path_buf() };
    fs::create_dir_all(layout.runtime(OLD)).unwrap();
    fs::write(layout.runtime(OLD).join("north"), "").unwrap();
    symlink(layout::link(OLD), layout.current()).unwrap();
    let command = super::seat::one_shot(&layout, super::seat::CANARY, NEW).unwrap();
    let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let runtime = layout.runtime(OLD).canonicalize().unwrap().display().to_string();
    let firn = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/firn/bin");
    assert_eq!(command.get_program(), "systemd-run");
    assert!(args.iter().any(|arg| arg == "--user") && args.iter().any(|arg| arg == "--slice=control.slice") && !args.iter().any(|arg| arg == "--scope"), "{args:?}");
    assert!(args.contains(&format!("--setenv=PATH={runtime}:{runtime}/tree/bin:{}:{}", super::seat::SYSTEM_BINS.join(":"), firn.display())), "{args:?}");
    assert_eq!(args[args.len() - 5..], [format!("{runtime}/north"), "runtime".to_owned(), "order-here".to_owned(), "canary".to_owned(), NEW.to_owned()]);
}
