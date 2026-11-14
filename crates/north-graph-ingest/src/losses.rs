//! What fixing each issue is measured to be worth: the 24 h usage of every harness cause class it owns, as
//! `north ledger flow` charges them (`loss-usd`), and what one of its boxes costs to finish (`box-usd`): the issue's own
//! worker usage per Done run over two weeks, else the fleet's. `north graph ready` ranks by their ratio (north#734).
use std::collections::BTreeMap;

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::run_roles::worker_where;
use north_graph::Term;
use north_base::foundation::clock::Timestamp;
use north_threads::threads::attribution::losses;
use rusqlite::{Connection, OpenFlags};

use crate::facts::{Facts, Sourced, issue_ref, text};
use crate::places::Places;

pub(crate) const WRITER: &str = "ledger";
const REPLACED: [&str; 2] = ["loss-usd", "box-usd"];
const DAY_S: i64 = 86_400;
const ESTIMATE_DAYS: i64 = 14;

/// Each harness class label and the issue that owns it, from `harness_loss_owners`.
fn owners() -> BTreeMap<String, String> {
    document(DEFAULTS)["harness_loss_owners"]["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pair| pair.as_str()?.split_once('='))
        .map(|(class, owner)| (class.trim().to_owned(), owner.trim().to_owned()))
        .collect()
}

pub fn read(places: &Places) -> Result<Sourced, String> {
    let (owners, now) = (owners(), Timestamp::now().unix_seconds());
    let db = Connection::open_with_flags(&places.threads, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| format!("{}: {why}", places.threads.display()))?;
    let reading = losses(&db, now - DAY_S).map_err(|why| format!("{}: losses: {why}", places.threads.display()))?;
    let mut owned: BTreeMap<String, f64> = BTreeMap::new();
    let mut facts = Facts::new(&REPLACED);
    for loss in reading.losses.iter().filter(|loss| loss.usd > 0.0) {
        match owners.get(loss.harness.label()) {
            Some(owner) => *owned.entry(owner.clone()).or_default() += loss.usd,
            None => facts.note(format!("harness class {} (${:.2} over 24 h) has no owner in harness_loss_owners", loss.harness.label(), loss.usd)),
        }
    }
    let per_box = per_done(&db, now - ESTIMATE_DAYS * DAY_S).map_err(|why| format!("{}: runs: {why}", places.threads.display()))?;
    let fleet = per_box.get("").copied();
    for (issue, usd) in owned {
        let Some(estimate) = per_box.get(&issue).copied().or(fleet) else {
            facts.note(format!("{issue} owns ${usd:.2} of harness losses, but no worker run finished Done in {ESTIMATE_DAYS} days, so a box has no measured cost"));
            continue;
        };
        facts.tell(WRITER, text(&issue), "loss-usd", Term::float(usd));
        facts.tell(WRITER, text(&issue), "box-usd", Term::float(estimate));
    }
    Ok(facts.finish_noted())
}

/// Worker usage per Done run since `since`, by issue, and the fleet's under the empty key.
fn per_done(db: &Connection, since: i64) -> rusqlite::Result<BTreeMap<String, f64>> {
    let mut statement = db.prepare(&format!("SELECT coalesce(item, ''), coalesce(cost_usd, 0), outcome = 'done' FROM runs WHERE {} AND outcome NOT IN ('running', 'idle') AND ended >= ?1", worker_where()))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, bool>(2)?)))?;
    let mut sums: BTreeMap<String, (f64, u32)> = BTreeMap::new();
    for (item, usd, done) in rows.filter_map(Result::ok) {
        for key in issue_ref(&item).into_iter().chain([String::new()]) {
            let sum = sums.entry(key).or_default();
            sum.0 += usd;
            sum.1 += u32::from(done);
        }
    }
    Ok(sums.into_iter().filter(|(_, (usd, done))| *done > 0 && *usd > 0.0).map(|(issue, (usd, done))| (issue, usd / f64::from(done))).collect())
}
