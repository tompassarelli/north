use std::cell::Cell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::client::North;
use crate::exec::{Clock, Wall};
use crate::notes::Notes;

use super::super::{Journey, Plan};
use super::hand_in;

const UNSETTLED: &str = r#"{"status":"working","conversation":"c","entries":[{"kind":"user","text":"the brief"}]}"#;

struct Virtual {
    start: Instant,
    at: Cell<Instant>,
}

impl Clock for Virtual {
    fn now(&self) -> Instant {
        self.at.get()
    }

    fn pause(&self, span: Duration) {
        self.at.set(self.at.get() + span);
    }
}

#[test]
fn a_hand_in_step_that_never_settles_fails_in_under_120_s_naming_the_step() {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("north");
    fs::write(&program, format!("#!/bin/sh\nprintf '%s\\n' '{UNSETTLED}'\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let state = root.path().to_path_buf();
    let north = North::new(program, state.clone(), state.clone(), None);
    let start = Wall.now();
    let clock = Rc::new(Virtual { start, at: Cell::new(start) });
    let mut journey = Journey { north, notes: Notes::new(None), plan: Plan { stamp: 0, claude_account: None, state }, session: "vn-0".to_owned(), scratch: None, lander: None, broker: None, landed: None, clock: clock.clone() };
    let failure = hand_in(&mut journey).unwrap_err();
    assert!(failure.starts_with("the hand-in step stopped inside its 110 s deadline"), "{failure}");
    assert!(failure.contains("vn-0 did not settle within 110 s; status working"), "{failure}");
    let waited = clock.at.get() - clock.start;
    assert!(waited < Duration::from_secs(120), "the step waited {waited:?} before it failed");
}
