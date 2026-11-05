use std::sync::Mutex;

use super::*;
use crate::error::NorthError;

#[derive(Default)]
struct Recorded {
    revived: Mutex<Vec<String>>,
    sent: Mutex<Vec<(String, String)>>,
    refuse: Option<&'static str>,
    out: Option<&'static str>,
}

impl Resumer for Recorded {
    fn revive(&self, name: &str) -> NorthResult<Revived> {
        if self.refuse == Some(name) {
            return Err(NorthError::Refused(format!("{name}: not signed in")));
        }
        if self.out == Some(name) {
            return Ok(Revived::Waiting("claude:c unavailable: exhausted: provider week 100%".into()));
        }
        self.revived.lock().unwrap().push(name.to_owned());
        Ok(Revived::Started)
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
fn a_session_whose_account_is_out_waits_untold_without_failing_the_run() {
    let resumer = Recorded { out: Some("sc-fl-surf"), ..Recorded::default() };

    let resumed = resume(&resumer, &names(&["sc-fl-surf", "wisp-api-g1"]), "whiterabbit", BOOTED_MS, Duration::from_secs(60));

    assert_eq!(resumed.lines(), ["resumed wisp-api-g1", "waiting sc-fl-surf: claude:c unavailable: exhausted: provider week 100%"]);
    assert!(resumed.failed.is_empty());
    assert_eq!(resumer.sent.lock().unwrap().iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["wisp-api-g1"]);
}

