//! The lander's judgement of a logged `land.judge` case, decided again by the code this binary links.
use north_strategy_types::land::{self, JudgeCase};

use super::bisect;
use super::checks::Verdict;
use super::steps::recorded_judgement;

pub fn judge(case: &JudgeCase) -> land::Judgement {
    recorded_judgement(&bisect::judge(verdict(&case.verdict), case.included.len(), case.base_moved))
}

fn verdict(recorded: &land::Verdict) -> Verdict {
    match recorded {
        land::Verdict::Green => Verdict::Green,
        land::Verdict::Refused(detail) => Verdict::Refused(detail.clone()),
        land::Verdict::RedMain(detail) => Verdict::RedMain(detail.clone()),
        land::Verdict::Transient(detail) => Verdict::Transient(detail.clone()),
        land::Verdict::BrokenBase(detail) => Verdict::BrokenBase(detail.clone()),
        land::Verdict::Deferred(detail) => Verdict::Deferred(detail.clone()),
    }
}
