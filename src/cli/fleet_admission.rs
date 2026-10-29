//! Holding a launch that placement refuses only because its host is saturated: with `--wait`, the
//! default for a launch a lead makes, it prints the reason and checks again until the host admits it.
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;

use super::fleet_placement::{Placed, Refusal};

const POLL: Duration = Duration::from_secs(60);
const LIMIT: Duration = Duration::from_secs(2 * 3600);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Waiting {
    pub poll: Duration,
    pub limit: Duration,
}

impl Waiting {
    /// `--wait` or `--no-wait` when given, else waiting when the launch's parent is a lead (a session with no parent).
    pub fn chosen(flag: Option<bool>, fleet: &Fleet, parent: Option<&str>) -> Option<Self> {
        let lead_launch = parent.is_some_and(|parent| fleet.levels_below_lead(parent) == 0);
        flag.unwrap_or(lead_launch).then_some(Self { poll: POLL, limit: LIMIT })
    }
}

/// Where the launch runs once placement admits it; `attempt(first)` runs placement, printing its
/// verdicts on the first call. A saturated host refuses at once without `waiting`, and after its limit with it.
pub(super) fn admitted(
    name: &str,
    waiting: Option<Waiting>,
    mut attempt: impl FnMut(bool) -> NorthResult<Result<Placed, Refusal>>,
    mut say: impl FnMut(&str),
) -> NorthResult<Placed> {
    let started = Instant::now();
    let mut first = true;
    loop {
        let why = match attempt(first)? {
            Ok(placed) => return Ok(placed),
            Err(Refusal::Saturated(why)) => why,
            Err(refusal) => return Err(refusal.into()),
        };
        let Some(waiting) = waiting else { return Err(NorthError::Refused(why)) };
        if started.elapsed() >= waiting.limit {
            return Err(NorthError::Refused(format!("{why}; still saturated after waiting {} min", waiting.limit.as_secs() / 60)));
        }
        say(&format!("fleet launch: {name} waits for admission: {why}; checking again in {} s", waiting.poll.as_secs()));
        std::thread::sleep(waiting.poll);
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick(limit: Duration) -> Option<Waiting> {
        Some(Waiting { poll: Duration::ZERO, limit })
    }

    fn queued() -> Result<Placed, Refusal> {
        Err(Refusal::Saturated("gex45 skipped: 9 queued for admission".into()))
    }

    #[test]
    fn a_waiting_launch_prints_the_queue_reason_until_admitted_and_then_starts_where_placement_put_it() {
        let mut answers = vec![queued(), queued(), Ok(Placed { note: Some("pin: x".into()), ..Placed::default() })].into_iter();
        let (mut firsts, mut said) = (Vec::new(), Vec::new());
        let placed = admitted("box", quick(Duration::from_secs(60)), |first| {
            firsts.push(first);
            Ok(answers.next().unwrap())
        }, |line| said.push(line.to_owned()))
        .unwrap();
        assert_eq!(placed.note.as_deref(), Some("pin: x"));
        assert_eq!(firsts, [true, false, false]);
        assert_eq!(said, ["fleet launch: box waits for admission: gex45 skipped: 9 queued for admission; checking again in 0 s"; 2]);
    }

    #[test]
    fn a_launch_is_refused_at_once_unless_it_waits_on_saturation_and_a_waiting_one_after_its_limit() {
        let mut said = Vec::new();
        let refused = admitted("box", None, |_| Ok(queued()), |line| said.push(line.to_owned())).unwrap_err();
        assert!(matches!(refused, NorthError::Refused(why) if why == "gex45 skipped: 9 queued for admission"));
        assert!(said.is_empty());
        let other = admitted("box", quick(Duration::from_secs(60)), |_| Ok(Err(Refusal::Final("claude:a unavailable".into()))), |_| {}).unwrap_err();
        assert!(matches!(other, NorthError::Refused(why) if why == "claude:a unavailable"));
        let expired = admitted("box", quick(Duration::ZERO), |_| Ok(queued()), |_| {}).unwrap_err();
        assert!(matches!(expired, NorthError::Refused(why) if why.ends_with("still saturated after waiting 0 min")));
    }
}
