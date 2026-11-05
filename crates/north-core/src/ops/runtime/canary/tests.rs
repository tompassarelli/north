use super::*;
use std::cell::RefCell;
use std::os::unix::fs::symlink;

const PREVIOUS: &str = "1111111111111111111111111111111111111111";
const PROMOTED: &str = "2222222222222222222222222222222222222222";

struct Fleet<'a> {
    layout: &'a Layout,
    exit: Ran,
    offloads: RefCell<u32>,
    alarms: RefCell<Vec<String>>,
}

impl Machine for Fleet<'_> {
    fn offload(&self, _: &str) -> Ran {
        *self.offloads.borrow_mut() += 1;
        match &self.exit {
            Ran::Exited { code, log } => Ran::Exited { code: *code, log: log.clone() },
            Ran::Unrun(why) => Ran::Unrun(why.clone()),
        }
    }

    fn roll_back(&self, sha: &str) -> Result<String, String> {
        update::roll_back(self.layout, sha).map_err(|failure| failure.message)
    }

    fn alarm(&self, text: &str) {
        self.alarms.borrow_mut().push(text.to_owned());
    }
}

struct Host {
    _dir: tempfile::TempDir,
    layout: Layout,
}

impl Host {
    fn promoted() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout { state: dir.path().to_path_buf() };
        for sha in [PREVIOUS, PROMOTED] {
            fs::create_dir_all(layout.runtime(sha)).unwrap();
            for program in layout::PROGRAMS {
                fs::write(layout.runtime(sha).join(program), "").unwrap();
            }
        }
        symlink(layout::link(PROMOTED), layout.current()).unwrap();
        fs::write(layout.previous(), PREVIOUS).unwrap();
        Self { _dir: dir, layout }
    }

    fn fleet(&self, exit: Ran) -> Fleet<'_> {
        Fleet { layout: &self.layout, exit, offloads: RefCell::new(0), alarms: RefCell::new(Vec::new()) }
    }

    fn current(&self) -> std::path::PathBuf {
        fs::read_link(self.layout.current()).unwrap()
    }
}

fn exited(code: u8, log: &str) -> Ran {
    Ran::Exited { code, log: log.to_owned() }
}

#[test]
fn a_promoted_runtime_that_fails_the_canary_land_check_is_rolled_back_with_an_alarm_naming_the_check() {
    let host = Host::promoted();
    let fleet = host.fleet(exited(127, "nix develop: exec loop\nbash: nix: command not found\n"));
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::RolledBack);
    assert_eq!(host.current(), layout::link(PREVIOUS));
    assert!(fs::read_to_string(host.layout.failed()).unwrap().starts_with(&format!("{PROMOTED} rollback ")));
    let alarms = fleet.alarms.borrow();
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].contains("land check `true` on gex45 exited 127"), "{}", alarms[0]);
    assert!(alarms[0].contains("bash: nix: command not found"), "{}", alarms[0]);
    assert!(alarms[0].contains("rolled back to 11111111"), "{}", alarms[0]);
    assert!(matches!(update::hold::verdict(&host.layout, PROMOTED, true), update::hold::Verdict::Held(_)));
    assert!(!host.layout.canaried().exists());
}

#[test]
fn a_promoted_runtime_that_passes_the_canary_changes_nothing_and_is_not_checked_again() {
    let host = Host::promoted();
    let fleet = host.fleet(exited(0, ""));
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::Passed);
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert!(!host.layout.failed().exists());
    assert!(fleet.alarms.borrow().is_empty());
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::Skipped);
    assert_eq!(*fleet.offloads.borrow(), 1);
    assert_eq!(check(&host.layout, "nexus", PROMOTED, &fleet), Verdict::Passed);
}

#[test]
fn a_canary_that_could_not_run_neither_rolls_back_nor_counts_as_judged() {
    let host = Host::promoted();
    let fleet = host.fleet(Ran::Unrun("no verdict within 300s".to_owned()));
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::Unrun("no verdict within 300s".to_owned()));
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert!(fleet.alarms.borrow().is_empty());
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::Unrun("no verdict within 300s".to_owned()));
    assert_eq!(*fleet.offloads.borrow(), 2);
}

#[test]
fn a_failed_canary_with_no_previous_runtime_alarms_once_and_is_not_retried() {
    let host = Host::promoted();
    fs::remove_file(host.layout.previous()).unwrap();
    let fleet = host.fleet(exited(2, "deadline: unknown form"));
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::Stuck);
    assert_eq!(host.current(), layout::link(PROMOTED));
    assert!(fleet.alarms.borrow()[0].contains("could not roll back"));
    assert_eq!(check(&host.layout, "gex45", PROMOTED, &fleet), Verdict::Skipped);
    assert_eq!(fleet.alarms.borrow().len(), 1);
}

#[test]
fn only_a_run_that_reached_a_slot_is_a_verdict_and_the_log_tail_is_bounded() {
    assert!(matches!(classify(Some(255), String::new()), Ran::Unrun(_)));
    assert!(matches!(classify(Some(96), "busy".into()), Ran::Unrun(_)));
    assert!(matches!(classify(None, String::new()), Ran::Unrun(_)));
    assert!(matches!(classify(Some(2), "bad".into()), Ran::Exited { code: 2, .. }));
    let long: String = (0..100).map(|n| format!("line {n}\n")).collect();
    let tail = tail(&long);
    assert!(tail.ends_with("line 99") && !tail.contains("line 80 |") && tail.len() <= TAIL_BYTES);
}

#[test]
fn a_roll_back_of_a_runtime_that_is_not_current_is_refused() {
    let host = Host::promoted();
    assert!(update::roll_back(&host.layout, PREVIOUS).is_err());
    assert_eq!(host.current(), layout::link(PROMOTED));
}
