use super::super::failure::Failure;
use super::super::layout::Layout;
use super::super::lifecycle::{self, Promoting, Promotion};
use north_base::foundation::clock::Timestamp;
use std::time::Duration;

const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) struct Hold {
    state: Promotion,
    reason: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) enum Verdict {
    Clear,
    Held(Hold),
    Waived(Hold),
}

impl Hold {
    fn word(&self) -> &'static str {
        match self.state {
            Promotion::BuildFailed => "build",
            Promotion::RolledBack => "rollback",
            _ => "promote",
        }
    }
}

pub(in super::super) fn verdict(layout: &Layout, target: &str, shipped: bool) -> Result<Verdict, Failure> {
    let Some(record) = lifecycle::record_of::<Promoting>(layout, target)? else {
        return Ok(Verdict::Clear);
    };
    let quarantined = record.state == Promotion::RolledBack;
    let lapsed = !quarantined && Timestamp::now().unix_millis() - record.since.unix_millis() >= i64::try_from(RETRY_AFTER.as_millis()).unwrap_or(i64::MAX);
    if !record.state.holds_back() || lapsed {
        return Ok(Verdict::Clear);
    }
    let hold = Hold { state: record.state, reason: record.cause };
    Ok(match shipped && hold.state == Promotion::BuildFailed {
        true => Verdict::Waived(hold),
        false => Verdict::Held(hold),
    })
}

impl Verdict {
    pub(super) fn notice(&self, short: &str, was: &str) -> Option<String> {
        match self {
            Verdict::Clear => None,
            Verdict::Held(hold) if hold.state == Promotion::RolledBack => Some(format!("north runtime: {short} is quarantined after it was rolled back; staying on {was} until a newer landing ({hold})")),
            Verdict::Held(hold) => Some(format!("north runtime: {short} failed within the last hour; staying on {was} ({hold})")),
            Verdict::Waived(hold) => Some(format!("north runtime: {short} failed within the last hour on this host ({hold}), but a runtime built elsewhere was shipped here; promoting it")),
        }
    }
}

impl std::fmt::Display for Hold {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason.is_empty() {
            true => write!(out, "{} failed", self.word()),
            false => write!(out, "{} failed: {}", self.word(), self.reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = "2edb782700000000000000000000000000000000";

    #[test]
    fn a_rolled_back_or_failed_promotion_holds_even_a_shipped_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout { state: dir.path().to_path_buf() };
        let reason = "failed step health check; exit status 1";
        lifecycle::move_to::<Promoting>(&layout, SHIPPED, Promotion::Installing, "shipped").unwrap();
        lifecycle::move_to::<Promoting>(&layout, SHIPPED, Promotion::InstallFailed, reason).unwrap();
        let Verdict::Held(held) = verdict(&layout, SHIPPED, true).unwrap() else { panic!("a runtime that failed after it was built stays held") };
        assert_eq!((held.state, held.reason.as_str()), (Promotion::InstallFailed, reason));

        lifecycle::move_to::<Promoting>(&layout, SHIPPED, Promotion::Installing, "retried").unwrap();
        lifecycle::move_to::<Promoting>(&layout, SHIPPED, Promotion::Live, "passed").unwrap();
        lifecycle::move_to::<Promoting>(&layout, SHIPPED, Promotion::RolledBack, reason).unwrap();
        let Verdict::Held(held) = verdict(&layout, SHIPPED, true).unwrap() else { panic!("a rolled-back runtime stays held") };
        assert_eq!(held.state, Promotion::RolledBack);
    }
}
