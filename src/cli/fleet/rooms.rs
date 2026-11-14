//! Each Claude account's week left in the order placement ranks the accounts, for the new-session form of the fleet screen.

use crate::foundation::clock::Timestamp;
use crate::tui::fleet::AccountRoom;
use crate::usage::outlook;

use crate::fleet::registry::Fleet;

pub(crate) fn of(fleet: &Fleet) -> Vec<AccountRoom> {
    let now = Timestamp::now();
    let Ok(verdicts) = crate::pacing::placement::case::verdicts(&fleet.roster()) else { return Vec::new() };
    let weeks = crate::usage::open_ledger(&crate::usage::profiles::home(), None, &|| {}).and_then(|db| outlook::claude_accounts(&db, &crate::platform::orchestration::path(), now)).unwrap_or_default();
    verdicts
        .iter()
        .filter_map(|verdict| verdict.account.strip_prefix("claude:"))
        .map(|label| {
            let used = weeks.iter().find(|account| account.account == label).and_then(|account| account.week_used);
            AccountRoom { label: label.to_owned(), week_left: used.map(|used| (100.0 - used).clamp(0.0, 100.0)) }
        })
        .collect()
}
