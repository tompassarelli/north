use std::time::Duration;

use north_base::lifecycle::{Lifecycle, State};

const KEEP_RECORDED: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Launched,
    Exited,
    Recorded,
}

impl State for Phase {
    const ALL: &'static [Self] = &[Self::Launched, Self::Exited, Self::Recorded];

    fn name(self) -> &'static str {
        match self {
            Self::Launched => "launched",
            Self::Exited => "exited",
            Self::Recorded => "recorded",
        }
    }
}

pub struct ShadowCheck;

impl Lifecycle for ShadowCheck {
    const NAME: &'static str = "shadow-check";
    type State = Phase;

    fn legal(from: Option<Phase>, to: Phase) -> bool {
        match from {
            None => to == Phase::Launched,
            Some(Phase::Launched) => to == Phase::Recorded,
            Some(Phase::Exited) => matches!(to, Phase::Recorded | Phase::Launched),
            Some(Phase::Recorded) => to == Phase::Launched,
        }
    }

    fn dwell(_: Phase) -> Option<Duration> {
        None
    }

    fn orphan(state: Phase) -> Option<Phase> {
        (state == Phase::Launched).then_some(Phase::Exited)
    }

    fn keep(state: Phase) -> Option<Duration> {
        (state == Phase::Recorded).then_some(KEEP_RECORDED)
    }
}
