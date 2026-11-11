use super::super::canary::{self, Machine, Ran, Rollback};
use super::super::failure::Failure;
use super::super::layout::{self, Layout};
use super::super::pin::{Event, read};
use super::{Host, Order, run};
use std::fs;
use std::os::unix::fs::symlink;
use std::sync::Mutex;

const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

struct Probes(Option<&'static str>);

impl Machine for Probes {
    fn probe(&self, probe: &canary::Probe) -> Ran {
        match self.0.is_some_and(|failing| probe.name.contains(failing)) {
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

    fn settles_fleet(&self, _host: &str) -> bool {
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
    live: Mutex<String>,
    pin: Mutex<Option<String>>,
    steps: &'a Mutex<Vec<String>>,
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
        canary::preflight(self.layout, self.name, sha, &Probes(self.failing))
    }

    fn go_live(&self, sha: &str) -> Result<(), Failure> {
        self.step("live");
        *self.live.lock().unwrap() = sha.to_owned();
        Ok(())
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
    let host = |name, failing| Rehearsal { layout: &layout, name, failing, live: Mutex::new(OLD.to_owned()), pin: Mutex::new(Some(OLD.to_owned())), steps: &steps };
    let (settling, builder) = (host("whiterabbit", None), host("gex45", Some("depbox")));
    let order = Order { sha: NEW.to_owned(), reason: "the lander cannot take a landing".to_owned(), orderer: "f-promote-order".to_owned() };

    let refused = run(&layout, &order, &settling, &[&builder], || Ok(())).unwrap_err();

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

    run(&layout, &order, &host, &[], || {
        host.note("hold restored");
        Ok(())
    })
    .unwrap();

    let held = held.lock().unwrap();
    assert!(held.len() >= 4 && held.iter().all(|step| step.ends_with("promotion lock held")), "{held:?}");
    assert!(super::super::update::promotion_waiting(&layout, std::time::Duration::ZERO, "after the order").is_ok(), "the order lets go of the lock when it ends");
}
