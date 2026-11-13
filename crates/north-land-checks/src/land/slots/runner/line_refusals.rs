use std::time::{Duration, Instant};

use super::say;
use north_base::foundation::offload::LAND_LINE_DEADLINE_SECONDS;

/// The longest one hold lasts once every build host's line has refused a run, before it asks again.
const HOLD: Duration = Duration::from_secs(60);
/// How many times one run places again after a build host's slot line refused it.
const REPLACEMENTS: usize = 12;

/// The build hosts whose slot lines refused a run at once, because each expected a wait past its deadline, and how long the run has been in lines in all.
pub(super) struct Refusals {
    since: Instant,
    budget: Duration,
    hold: Duration,
    hosts: Vec<String>,
    asks: usize,
}

impl Refusals {
    pub(super) fn new() -> Self {
        Self { since: Instant::now(), budget: Duration::from_secs(LAND_LINE_DEADLINE_SECONDS), hold: HOLD, hosts: Vec::new(), asks: 0 }
    }

    /// The hosts the next placement leaves out: those that failed and those whose line refused.
    pub(super) fn avoiding(&self, failed: &[String]) -> Vec<String> {
        failed.iter().chain(&self.hosts).cloned().collect()
    }

    /// The seconds left of the one line deadline since the first placement, which the next placement hands its job as the most it may wait in a line.
    pub(super) fn left(&self) -> u64 {
        self.budget.saturating_sub(self.since.elapsed()).as_secs()
    }

    /// What follows a refusal by `host`'s line: another placement at once, on the host with the least expected wait among those not yet refused, or after a hold once every host has refused; an error with the seconds spent once the budget or `REPLACEMENTS` placements are used up.
    pub(super) async fn place_again(&mut self, host: &str) -> Result<(), String> {
        self.asks += 1;
        let spent = self.since.elapsed();
        if self.asks > REPLACEMENTS || spent >= self.budget {
            return Err(format!("every build host's slot line expected the checks to wait past its deadline, and {} s in lines since the first placement leaves no time to wait again", spent.as_secs()));
        }
        if self.hosts.iter().any(|refused| refused == host) {
            let hold = self.hold.min(self.budget - spent);
            say(&format!("safe-push: every build host's slot line expects the checks to wait past its deadline; holding {} s before asking again, {} s of {} s spent", hold.as_secs(), spent.as_secs(), self.budget.as_secs()));
            tokio::time::sleep(hold).await;
            self.hosts.clear();
            return Ok(());
        }
        say(&format!("safe-push: the slot line of {} expects the checks to wait past its deadline; placing them on the build host with the least expected wait", if host.is_empty() { "this host" } else { host }));
        if !host.is_empty() {
            self.hosts.push(host.to_owned());
        }
        Ok(())
    }
}
