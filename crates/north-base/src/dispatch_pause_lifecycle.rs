use std::time::Duration;

use crate::lifecycle::{Lifecycle, State};

const EXPIRY_DWELL: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Held,
    Checked,
    Expiring,
    Lifted,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Held, Self::Checked, Self::Expiring, Self::Lifted];

    fn name(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Checked => "checked",
            Self::Expiring => "expiring",
            Self::Lifted => "lifted",
        }
    }
}

pub struct DispatchPause;

impl Lifecycle for DispatchPause {
    const NAME: &'static str = "dispatch-pause";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        match from {
            None => to != Phase::Lifted,
            Some(Phase::Lifted) => false,
            Some(_) => true,
        }
    }

    fn dwell(state: Phase) -> Option<Duration> {
        (state == Phase::Expiring).then_some(EXPIRY_DWELL)
    }

    fn orphan(_: Phase) -> Option<Phase> {
        None
    }
}
