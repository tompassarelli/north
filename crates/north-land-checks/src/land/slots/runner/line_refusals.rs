use std::time::{Duration, Instant};

use super::say;
use north_base::foundation::offload::LAND_LINE_DEADLINE_SECONDS;

/// The longest one hold lasts once every build host's line has refused a run, before it asks again.
const HOLD: Duration = Duration::from_secs(60);

/// The build hosts whose slot lines refused a run at once, because each expected a wait past its deadline, and when the current line window of the run began.
pub(super) struct Refusals {
    since: Instant,
    budget: Duration,
    hold: Duration,
    hosts: Vec<String>,
}

impl Refusals {
    pub(super) fn new() -> Self {
        Self::within(LAND_LINE_DEADLINE_SECONDS)
    }

    /// Refusals whose placements share a line window of `seconds`, which a refusal starts over once spent.
    pub(super) fn within(seconds: u64) -> Self {
        Self { since: Instant::now(), budget: Duration::from_secs(seconds), hold: HOLD, hosts: Vec::new() }
    }

    /// The hosts the next placement leaves out: those that failed and those whose line refused.
    pub(super) fn avoiding(&self, failed: &[String]) -> Vec<String> {
        failed.iter().chain(&self.hosts).cloned().collect()
    }

    /// The seconds left of the current line window, which the next placement hands its job as the most it may wait in a line.
    pub(super) fn left(&self) -> u64 {
        self.budget.saturating_sub(self.since.elapsed()).as_secs()
    }

    /// What follows a refusal with `status` by `host`'s line: another placement at once, on the host with the least expected wait among those not yet refused, or after a hold once every host has refused. A spent window starts over, so a refusal never ends the run.
    pub(super) async fn place_again(&mut self, host: &str, status: i32) {
        if self.since.elapsed() >= self.budget {
            self.since = Instant::now();
        }
        if host.is_empty() || self.hosts.iter().any(|refused| refused == host) {
            say(&format!("safe-push: exit {status} from every build host's slot line, which expect the checks to wait past its deadline; holding {} s before asking again", self.hold.as_secs()));
            tokio::time::sleep(self.hold).await;
            self.hosts.clear();
            return;
        }
        say(&format!("safe-push: exit {status} from the slot line of {}, which expects the checks to wait past its deadline; placing them on the build host with the least expected wait", if host.is_empty() { "this host" } else { host }));
        if !host.is_empty() {
            self.hosts.push(host.to_owned());
        }
    }
}
