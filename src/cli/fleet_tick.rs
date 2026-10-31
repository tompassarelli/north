use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::store::OverSsh;
use crate::fleet::tick::{FarHost, Pulls, tick};
use crate::ops::pacing::placement::{config, gate};

use super::fleet_ledger::Ledger;
use super::fleet_rebooted;
use super::fleet_waits::Probes;

const USAGE: &str = "Usage: north fleet tick";
const BUDGET: Duration = Duration::from_secs(15);

fn always_pulled(fleet: &Fleet) -> Vec<FarHost> {
    config().map(|cfg| gate::spool_pulls(&cfg, fleet.host())).unwrap_or_default().into_iter().map(|(name, ssh)| FarHost { name, ssh }).collect()
}

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    if !words.is_empty() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    let pulls = Pulls { spools: &OverSsh::default(), always: always_pulled(fleet) };
    let ticked = tick(fleet, &mut Ledger, &pulls, &Probes, now_ms(), BUDGET);
    let revived = fleet_rebooted::run(fleet, &["--revive"]);
    ticked?.lines().iter().for_each(|line| println!("{line}"));
    revived
}
