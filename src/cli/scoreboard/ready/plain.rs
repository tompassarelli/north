//! Each bar's name in words a reader needs no statistics for, as `north dash` prints it.
use super::Bar;

impl Bar {
    pub(super) fn plain(self) -> &'static str {
        match self {
            Self::LandingP90 => "landing wait, slowest 1 in 10",
            Self::VerificationP50 => "checks after landing, typical",
            Self::VerificationP90 => "checks after landing, slowest 1 in 10",
            Self::Rework => "landed work redone within 48 h",
            Self::FirstTry => "first tries that land",
            Self::HarnessLoss => "first tries lost to tooling",
            Self::AskToLanding => "ask to landed, typical",
            Self::Corrections => "Tom's corrections per hour",
            Self::PriorityNow => "urgent issues open",
            Self::IdleAccount => "longest an account sat idle",
            Self::RuntimeBreaks => "broken runtimes that reached a second host",
            Self::RuntimeRollback => "slowest rollback of a broken runtime",
            Self::RuntimeLive => "how long a landing took to go live on the fleet's runtime",
            Self::RuntimeLag => "how far the slowest process on any host ran behind the newest landing",
            Self::FirnClosureLive => "how long a landing took to reach every host's system closure",
            Self::UsagePerLanding => "usage per first try landed",
            Self::StrandedUsage => "usage wasted while work waited",
            Self::MainGreenLander => "main green, landing checks",
            Self::MainGreenCi => "main green, GitHub checks",
        }
    }
}
