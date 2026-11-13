//! Each bar's name in words a reader needs no statistics for, as `north dash` prints it.
use super::bar::Bar;

const PLAIN: [(Bar, &str); 22] = [
    (Bar::LandingP90, "landing wait, slowest 1 in 10"),
    (Bar::VerificationP50, "checks after landing, typical"),
    (Bar::VerificationP90, "checks after landing, slowest 1 in 10"),
    (Bar::Rework, "landed work redone within 48 h"),
    (Bar::FirstTry, "first tries that land"),
    (Bar::HarnessLoss, "first tries lost to tooling"),
    (Bar::AskToLanding, "ask to landed, typical"),
    (Bar::Corrections, "Tom's corrections per hour"),
    (Bar::PriorityNow, "urgent issues open"),
    (Bar::IdleAccount, "longest an account sat idle"),
    (Bar::RuntimeBreaks, "broken runtimes that reached a second host"),
    (Bar::RuntimeRollback, "slowest rollback of a broken runtime"),
    (Bar::RuntimeLive, "how long a due promotion took to go live on the fleet's runtime"),
    (Bar::RuntimeCanary, "how long the canary took to judge a runtime on one host"),
    (Bar::RuntimeShip, "how long a runtime took to copy to a host"),
    (Bar::RuntimeAge, "how long the oldest landing has waited for the fleet's runtime"),
    (Bar::RuntimeLag, "how far the slowest process on any host ran behind the newest landing"),
    (Bar::FirnClosureLive, "how long a landing took to reach every host's system closure"),
    (Bar::UsagePerLanding, "usage per first try landed"),
    (Bar::StrandedUsage, "usage wasted while work waited"),
    (Bar::MainGreenLander, "main green, landing checks"),
    (Bar::MainGreenCi, "main green, GitHub checks"),
];

impl Bar {
    pub(super) fn plain(self) -> &'static str {
        PLAIN.iter().find(|(bar, _)| *bar == self).map(|(_, plain)| *plain).expect("every bar has its plain name")
    }
}
