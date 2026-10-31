use std::sync::Mutex;

use super::*;
use crate::error::NorthError;

#[derive(Default)]
struct Recorded {
    revived: Mutex<Vec<String>>,
    sent: Mutex<Vec<(String, String)>>,
    refuse: Option<&'static str>,
}

impl Resumer for Recorded {
    fn revive(&self, name: &str) -> NorthResult<()> {
        if self.refuse == Some(name) {
            return Err(NorthError::Refused(format!("{name}: account is out")));
        }
        self.revived.lock().unwrap().push(name.to_owned());
        Ok(())
    }

    fn send(&self, name: &str, text: &str) -> NorthResult<()> {
        self.sent.lock().unwrap().push((name.to_owned(), text.to_owned()));
        Ok(())
    }
}

const BOOTED_MS: u64 = 1_793_417_760_000;

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn each_stranded_session_is_revived_then_told_which_host_rebooted_and_when() {
    let resumer = Recorded::default();

    let resumed = resume(&resumer, &names(&["sc-789-g", "wisp-api-g1"]), "gex45", BOOTED_MS, Duration::from_secs(60));

    let text = "host gex45 rebooted at 2026-10-31T03:36Z; resume from your last checkpoint";
    assert_eq!(*resumer.revived.lock().unwrap(), ["sc-789-g", "wisp-api-g1"]);
    assert_eq!(*resumer.sent.lock().unwrap(), [("sc-789-g".to_owned(), text.to_owned()), ("wisp-api-g1".to_owned(), text.to_owned())]);
    assert_eq!(resumed.lines(), ["resumed sc-789-g", "resumed wisp-api-g1"]);
}

#[test]
fn a_session_that_cannot_be_revived_is_not_told_and_leaves_the_others_resumed() {
    let resumer = Recorded { refuse: Some("sc-789-g"), ..Recorded::default() };

    let resumed = resume(&resumer, &names(&["sc-789-g", "wisp-api-g1"]), "gex45", BOOTED_MS, Duration::from_secs(60));

    assert_eq!(resumed.lines(), ["resumed wisp-api-g1", "not resumed sc-789-g: sc-789-g: account is out"]);
    assert_eq!(resumer.sent.lock().unwrap().iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["wisp-api-g1"]);
}

#[test]
fn a_spent_budget_defers_the_rest_to_the_next_run() {
    let resumer = Recorded::default();

    let resumed = resume(&resumer, &names(&["a", "b"]), "gex45", BOOTED_MS, Duration::ZERO);

    assert_eq!(resumed.lines(), ["deferred a to the next run", "deferred b to the next run"]);
    assert!(resumer.revived.lock().unwrap().is_empty());
}
