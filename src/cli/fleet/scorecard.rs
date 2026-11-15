//! The fleet screen's top line: what landed since local midnight, how many of the workers started since then landed first try, and north's time and cost against the rival in `docs/readme/comparison.toml`.
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;
use crate::tui::fleet::{Comparison, FirstTry, Scorecard};

pub(crate) fn of(fleet: &Fleet) -> Scorecard {
    let now = Timestamp::now();
    let since_ms = now.local_day_start_ms();
    let first_try = crate::cli::dash::first_tries(i64::try_from(since_ms / 1000).unwrap_or(0), now.unix_seconds()).ok().map(|attempts| FirstTry { landed: attempts.iter().filter(|attempt| attempt.succeeded).count(), started: attempts.len() });
    Scorecard {
        landings: fleet.digest(since_ms).ok().map(|digest| digest.landed.len()),
        first_try,
        comparison: north_docs::headline().ok().map(|headline| Comparison { rival: headline.rival, time: headline.time, cost: headline.cost }),
    }
}
