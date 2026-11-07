use std::collections::BTreeSet;

use super::{ScratchUse, verdict};

const NOW: u64 = 1_793_359_614;

fn scratch(caller: &str, pusher: Option<&str>, age_s: u64, live: &[&str]) -> ScratchUse {
    ScratchUse {
        caller: Some(caller.to_owned()),
        pusher: pusher.map(str::to_owned),
        last_active_s: Some(NOW - age_s),
        now_s: NOW,
        live: Some(live.iter().map(|name| (*name).to_owned()).collect::<BTreeSet<_>>()),
    }
}

#[test]
fn a_scratch_branch_goes_when_its_caller_pushed_it_it_idled_a_day_or_its_pusher_exited_and_stays_for_another_live_session() {
    assert!(verdict(&scratch("north-371", Some("north-371"), 60, &["north-371"])).is_ok());
    assert!(verdict(&scratch("north-371", Some("client-cleanroom"), 60, &["north-371"])).is_ok());
    assert!(verdict(&scratch("north-371", None, 86_400, &[])).is_ok());
    assert!(verdict(&scratch("north-371", Some("client-cleanroom"), 60, &["client-cleanroom"])).is_err());
    assert!(verdict(&scratch("north-371", Some("client-cleanroom"), 86_399, &["client-cleanroom"])).is_err());
    assert!(verdict(&scratch("north-371", None, 60, &[])).is_err());
    assert!(verdict(&scratch("", Some(""), 60, &[])).is_err());
    let unread = ScratchUse { live: None, ..scratch("north-371", Some("client-cleanroom"), 60, &[]) };
    assert!(verdict(&unread).is_err());
    let unreadable = ScratchUse { last_active_s: None, ..scratch("north-371", None, 0, &[]) };
    assert!(verdict(&unreadable).is_err());
}
