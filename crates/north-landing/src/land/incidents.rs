use north_base::platform::attribution::{Cause, Harness};

pub fn class(verdict: &str) -> Option<&'static str> {
    if verdict.contains("deadline") || verdict.contains(" hung") {
        return Some("lander-hang");
    }
    if verdict.contains("persistent transient") || verdict.contains("persistent deferred") {
        return Some("lander-transient");
    }
    match verdict.split_whitespace().next() {
        Some("failed") => Some("lander-refused"),
        Some("conflict") => Some("lander-conflict"),
        Some("transient") => Some("lander-transient"),
        Some("deferred") => Some("lander-deferred"),
        Some("withdrawn") => Some("lander-withdrawn"),
        _ => None,
    }
}

/// The side a lander verdict belongs to: a failing check or a conflict in the lane's own lines is the worker's; a deferral, a hang, a transient error or an origin that refused the push is the harness's.
pub fn blame(verdict: &str) -> Option<Cause> {
    if verdict.starts_with("failed origin refused") {
        return Some(Cause::Harness(Harness::LandingRefusal));
    }
    match class(verdict)? {
        "lander-refused" | "lander-conflict" => Some(Cause::Worker),
        "lander-transient" | "lander-deferred" | "lander-hang" => Some(Cause::Harness(Harness::LandingRefusal)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lane_refused_for_its_own_failing_check_is_recorded_worker_caused() {
        assert_eq!(blame("failed /tmp/land/check.log"), Some(Cause::Worker));
        assert_eq!(blame("transient capacity"), Some(Cause::Harness(Harness::LandingRefusal)));
        assert_eq!(blame("failed origin refused the push of abc: hook declined"), Some(Cause::Harness(Harness::LandingRefusal)));
    }
}
