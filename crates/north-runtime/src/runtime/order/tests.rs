use super::super::canary::{self, Machine, Ran, Rollback};
use super::super::failure::Failure;
use super::super::layout::{self, Layout};
use super::super::lifecycle::{self, Canarying};
use super::super::pin::{Event, read};
use super::super::after_landing::Landing;
use super::super::review_hold::{Ask, Caller, Holds, Screen, in_checkout, landed_in, lane_of, now};
use super::super::update::{self, Flight};
use super::{Host, Order, run};
use std::fs;
use std::os::unix::fs::symlink;
use std::sync::Mutex;

const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";
const NEWER: &str = "3333333333333333333333333333333333333333";
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
        Ok(())
    }

    fn canary(&self, sha: &str) -> Result<(), String> {
        self.step("canary");
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
fn an_ordered_promotion_of_a_landed_commit_runs_the_canary_then_goes_live_on_the_settling_host() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    for sha in [OLD, NEW] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(OLD), layout.current()).unwrap();
    let steps = Mutex::new(Vec::new());
    let host = |name, failing| Rehearsal { layout: &layout, name, failing, retired: None, live: Mutex::new(OLD.to_owned()), pin: Mutex::new(Some(OLD.to_owned())), steps: &steps, stays: false };
    let (settling, builder) = (host("whiterabbit", None), host("gex45", Some("depbox")));
    let order = Order { sha: NEW.to_owned(), reason: "the lander cannot take a landing".to_owned(), orderer: "f-promote-order".to_owned() };

    let hand_pin = layout.state.join(super::PIN);
    fs::create_dir_all(hand_pin.parent().unwrap()).unwrap();
    fs::write(&hand_pin, format!("{OLD}\n")).unwrap();

    let refused = run(&layout, &order, &settling, &[&builder], &["nexus".to_owned()], || Ok(()), |()| super::hold::lift(&layout)).unwrap_err();

    assert!(!hand_pin.exists(), "with runtime_promotion = landing the order clears the hand pin and leaves north-stable-hold stopped, so later landings promote on their own");
    assert!(refused.message.contains("gex45 stays on its old runtime") && refused.message.contains("depbox"), "{}", refused.message);
    let steps = steps.lock().unwrap();
    let position = |what: &str| steps.iter().position(|step| step == what).unwrap_or_else(|| panic!("{what} never ran: {steps:?}"));
    assert!(position("whiterabbit canary") < position("whiterabbit live"));
    assert!(!steps.contains(&"gex45 live".to_owned()), "{steps:?}");
    assert_eq!((settling.live.lock().unwrap().as_str(), builder.live.lock().unwrap().as_str()), (NEW, OLD));
    assert_eq!((settling.pin.lock().unwrap().as_deref(), builder.pin.lock().unwrap().as_deref()), (Some(NEW), Some(OLD)));
    let rows: Vec<(String, String, bool)> = read(&layout.state).into_iter().filter_map(|event| match event {
        Event::Ordered { sha, host, orderer, reason, canary, live, .. } => {
            assert_eq!((sha.as_str(), orderer.as_str(), reason.as_str()), (NEW, "f-promote-order", "the lander cannot take a landing"));
            Some((host, canary, live))
        }
        _ => None,
    }).collect();
    let row = |host: &str| rows.iter().find(|row| row.0 == host).unwrap_or_else(|| panic!("no row for {host}: {rows:?}"));
    assert!(row("whiterabbit").1 == "passed" && row("whiterabbit").2);
    assert!(row("gex45").1.starts_with("failed: probe `a worker command's depbox run") && !row("gex45").2);
    assert!(rows.iter().all(|row| row.0 != "nexus") && steps.iter().all(|step| !step.starts_with("nexus")), "a host left out is shipped, judged and promoted nothing: {steps:?}");
    let left_out: Vec<(String, String)> = read(&layout.state).into_iter().filter_map(|event| match event {
        Event::LeftOut { host, orderer, reason, .. } => Some((host, format!("{orderer}: {reason}"))),
        _ => None,
    }).collect();
    assert_eq!(left_out, [("nexus".to_owned(), "f-promote-order: the lander cannot take a landing".to_owned())]);
    assert!(super::super::pin::status(&layout.state, None).contains("held: nexus"));
    let deployment = crate::land_offload::Deployment { destinations: vec!["tom@gex45".to_owned(), "nexus".to_owned()], names: vec!["gex45".to_owned(), "nexus".to_owned()], ..Default::default() };
    let named = |except: &[&str]| super::leave_out(&deployment, &except.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>(), "whiterabbit");
    assert_eq!(named(&["nexus"]).unwrap(), (vec!["tom@gex45".to_owned()], vec!["nexus".to_owned()]));
    assert!(named(&["whiterabbit"]).is_err() && named(&["nowhere"]).is_err() && named(&["gex45", "nexus"]).is_err());
}

struct Contended<'a> {
    layout: &'a Layout,
    held: &'a Mutex<Vec<String>>,
}

impl Contended<'_> {
    fn note(&self, step: &str) {
        let free = super::super::update::promotion_waiting(self.layout, std::time::Duration::ZERO, "an automatic promotion").is_ok();
        self.held.lock().unwrap().push(format!("{step}: {}", if free { "promotion lock free" } else { "promotion lock held" }));
    }
}

impl Host for Contended<'_> {
    fn name(&self) -> &str {
        "whiterabbit"
    }

    fn pinned(&self) -> Result<bool, Failure> {
        Ok(false)
    }

    fn acquire(&self, _sha: &str) -> Result<(), Failure> {
        self.note("build");
        Ok(())
    }

    fn canary(&self, _sha: &str) -> Result<(), String> {
        self.note("canary");
        Ok(())
    }

    fn go_live(&self, _sha: &str) -> Result<(), Failure> {
        self.note("go live");
        Ok(())
    }

    fn live(&self) -> Option<String> {
        Some(NEW.to_owned())
    }

    fn write_pin(&self, _sha: &str) -> Result<(), Failure> {
        Ok(())
    }
}

#[test]
fn an_order_holds_the_promotion_lock_while_its_canary_runs() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let held = Mutex::new(Vec::new());
    let host = Contended { layout: &layout, held: &held };
    let order = Order { sha: NEW.to_owned(), reason: "the lander cannot take a landing".to_owned(), orderer: "f-runtime-tree".to_owned() };

    run(&layout, &order, &host, &[], &[], || Ok(()), |()| {
        host.note("hold restored");
        Ok(false)
    })
    .unwrap();

    let held = held.lock().unwrap();
    assert!(held.len() >= 4 && held.iter().all(|step| step.ends_with("promotion lock held")), "{held:?}");
    assert!(super::super::update::promotion_waiting(&layout, std::time::Duration::ZERO, "after the order").is_ok(), "the order lets go of the lock when it ends");
}

#[test]
fn a_promotion_that_takes_the_lock_between_the_holds_release_and_the_order_leaves_it_to_the_order() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let held = Mutex::new(Vec::new());
    let host = Contended { layout: &layout, held: &held };
    let order = Order { sha: NEW.to_owned(), reason: "the lander cannot take a landing".to_owned(), orderer: "f-canary-chain".to_owned() };
    let mut between = None;

    run(
        &layout,
        &order,
        &host,
        &[],
        &[],
        || {
            between = Some(update::flight(&layout, &mut || panic!("a promotion inside an order's window promoted")));
            Ok(())
        },
        |()| Ok(false),
    )
    .unwrap();

    assert!(matches!(between, Some(Ok(Flight::Joined))), "on 2026-11-12 a landing's promotion took the lock the stopped hold had freed and put a commit above the ordered one live for 24 s");
    assert!(matches!(update::named_lock(&layout, "runtime-order.lock", rustix::fs::FlockOperation::NonBlockingLockExclusive), Ok(Some(_))), "the order's mark ends with the order");
}

#[test]
fn an_order_names_a_host_kept_on_a_newer_runtime_and_does_not_report_it_live() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    for sha in [OLD, NEW] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(OLD), layout.current()).unwrap();
    let steps = Mutex::new(Vec::new());
    let settling = Rehearsal { layout: &layout, name: "whiterabbit", failing: None, retired: None, live: Mutex::new(OLD.to_owned()), pin: Mutex::new(None), steps: &steps, stays: false };
    let kept = Rehearsal { layout: &layout, name: "gex45", failing: None, retired: None, live: Mutex::new(NEWER.to_owned()), pin: Mutex::new(Some(NEWER.to_owned())), steps: &steps, stays: true };
    let order = Order { sha: NEW.to_owned(), reason: "main's head holds a change under review".to_owned(), orderer: "f-canary-chain".to_owned() };

    let refused = run(&layout, &order, &settling, &[&kept], &[], || Ok(()), |()| Ok(false)).unwrap_err();

    assert!(refused.message.contains("gex45") && refused.message.contains("is not live") && refused.message.contains(layout::short(NEWER)), "on 2026-11-12 an order reported four hosts live on its commit that had stayed on a newer one: {}", refused.message);
    assert_eq!(kept.pin.lock().unwrap().as_deref(), Some(NEWER), "a host that is not on the ordered commit keeps its stable pin");
    let row = read(&layout.state).into_iter().find_map(|event| match event {
        Event::Ordered { host, canary, live, .. } if host == "gex45" => Some((canary, live)),
        _ => None,
    });
    assert!(row.as_ref().is_some_and(|(canary, live)| canary.starts_with("passed; not live: gex45 runs") && !live), "{row:?}");
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
    let host = |name, failing| Rehearsal { layout: &layout, name, failing, retired: Some("depbox"), live: Mutex::new(OLD.to_owned()), pin: Mutex::new(None), steps: &steps, stays: false };
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
    Ok(commit == SQUASHED && head == NEW)
}

fn landings(lane: &str) -> Result<Vec<String>, String> {
    Ok(if lane == "f-fix" { vec![SQUASHED.to_owned()] } else { Vec::new() })
}

fn screen_of(holds: Holds, contains: fn(&str, &str) -> Result<bool, String>, landed: fn(&str) -> Result<Vec<String>, String>) -> Screen {
    Screen { holds, contains: Box::new(contains), landed: Box::new(landed), after_landing: Box::new(|_| Landing::Green) }
}

#[test]
fn a_reviewers_hold_refuses_an_order_and_the_promotion_of_a_head_containing_it_until_the_reviewer_lifts_it() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    for sha in [OLD, NEW] {
        fs::create_dir_all(layout.runtime(sha)).unwrap();
    }
    symlink(layout::link(OLD), layout.current()).unwrap();
    let steps = Mutex::new(Vec::new());
    let settling = Rehearsal { layout: &layout, name: "whiterabbit", failing: None, retired: None, live: Mutex::new(OLD.to_owned()), pin: Mutex::new(None), steps: &steps, stays: false };
    let store = state.path().join("holds");
    let screen = screen_of(Holds(store.clone()), squashed, landings);
    let cleared = |sha: &str| screen.order(sha, &["whiterabbit"], now());
    let ordered = || Order::cleared(NEW.to_owned(), "a reviewed pipeline fix".to_owned(), "f-flow-lead".to_owned(), cleared);
    let session = |name: &str, coordinator| Caller { session: name.to_owned(), coordinator };
    let (reviewer, author, coordinator) = (session("f-review", false), session("f-holds", false), session("proxy", true));
    let place = |caller: &Caller| screen.holds.serve(Ask::Place { commit: REVIEWED.to_owned(), lane: "f-fix".to_owned(), host: None, why: "the canary reads the wrong lock".to_owned() }, caller, now());
    let lift = |caller: &Caller| screen.holds.serve(Ask::Lift { commit: REVIEWED.to_owned(), why: "the fix is reviewed".to_owned() }, caller, now());
    place(&reviewer).unwrap();

    let refused = ordered().err().map(|failure| failure.message).unwrap_or_default();
    assert!(refused.contains(REVIEWED) && refused.contains("f-review"), "a reviewer's hold was a message to the coordinator, who ordered each runtime by hand, and the lander lands the held lane as a squashed commit of its own: {refused:?}");
    let promotion = update::slot::held_by_review(&layout, &screen, NEW).unwrap_or_default();
    assert!(promotion.contains(REVIEWED) && promotion.contains("f-review"), "the slot and Live-fix promotion refuses the same head: {promotion:?}");
    for (settler, case) in [(Ok(true), "another host settles the fleet"), (Err("no such file".to_owned()), "its orchestration configuration cannot be read")] {
        assert!(update::slot::screened(&layout, state.path(), NEW.to_owned(), false, settler, &screen).is_none(), "a host that follows origin/main itself keeps no holds and no push ledger, so it takes no head of its own when {case}");
    }

    let not_theirs = lift(&author).unwrap_err();
    assert!(not_theirs.contains(REVIEWED) && not_theirs.contains("f-review"), "{not_theirs}");
    assert!(ordered().is_err(), "a lift by another session leaves the hold standing");

    lift(&reviewer).unwrap();
    run(&layout, &ordered().unwrap(), &settling, &[], &[], || Ok(()), |()| Ok(false)).unwrap();
    assert_eq!(settling.live.lock().unwrap().as_str(), NEW);
    assert!(update::slot::held_by_review(&layout, &screen, NEW).is_none());

    place(&reviewer).unwrap();
    lift(&coordinator).unwrap();
    let named = cleared(NEW).unwrap();
    assert!(named.iter().any(|line| line.contains("f-review") && line.contains("proxy lifted it: the fix is reviewed")), "{named:?}");
    let unresolved = screen_of(Holds(store.clone()), |_, _| Err("fatal: Not a valid object name".to_owned()), landings);
    assert!(unresolved.order(NEW, &["whiterabbit"], now()).is_ok(), "on 2026-11-13 a lifted hold on a commit the checkout lacks refused every order and promotion for a day");
    place(&reviewer).unwrap();

    let (unlisted, unparsed, misnamed) = (state.path().join("not-a-store"), state.path().join("unparsed"), state.path().join("misnamed"));
    fs::write(&unlisted, "").unwrap();
    for store in [&unparsed, &misnamed] {
        fs::create_dir_all(store).unwrap();
    }
    fs::write(unparsed.join("broken.json"), "{").unwrap();
    fs::copy(store.join(format!("{REVIEWED}.f-review.json")), misnamed.join(format!("{NEW}.f-review.json"))).unwrap();
    let failing: [(&str, Screen, &str); 5] = [
        ("a store that cannot be listed", screen_of(Holds(unlisted), squashed, landings), "not-a-store"),
        ("an entry that is no hold", screen_of(Holds(unparsed), squashed, landings), "broken.json"),
        ("an entry not named for its hold", screen_of(Holds(misnamed), squashed, landings), "not the hold its name says"),
        ("git failing", screen_of(Holds(store.clone()), |_, _| Err("fatal: no such object".to_owned()), landings), "no such object"),
        ("a push ledger that does not exist", Screen { holds: Holds(store), contains: Box::new(|_, _| Ok(false)), landed: landed_in(state.path().join("pushes.tsv")), after_landing: Box::new(|_| Landing::Green) }, "pushes.tsv"),
    ];
    for (case, screen, says) in failing {
        let refused = screen.order(NEW, &["whiterabbit"], now()).map_or_else(|why| why, |lines| format!("passed: {lines:?}"));
        assert!(refused.contains("does not go live") && refused.contains(says), "{case}: {refused}");
    }
    let ledger = format!("1794548552\tnorth\tf-fix\t{REVIEWED}\t{SQUASHED}\tf-fix\n");
    assert_eq!(lane_of(REVIEWED, &ledger, &[]), Ok("f-fix".to_owned()));
    for branches in [vec!["main".to_owned()], vec!["f-fix".to_owned(), "f-other".to_owned()]] {
        assert!(lane_of(REVIEWED, "", &branches).is_err(), "a hold whose lane is not one branch is refused at placement: {branches:?}");
    }
    let checkout = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| std::process::Command::new("git").current_dir(checkout.path()).args(arguments).output().unwrap();
    git(&["init", "-q"]);
    let absent = in_checkout(checkout.path(), REVIEWED).unwrap_err();
    assert!(absent.contains(REVIEWED) && absent.contains("no commit in the north checkout"), "a hold placed on a commit the checkout lacks made every later screen fail: {absent}");
    git(&["-c", "user.name=north", "-c", "user.email=north@invalid", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "reviewed"]);
    let head = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout).unwrap();
    assert_eq!(in_checkout(checkout.path(), head.trim()), Ok(()));
}
