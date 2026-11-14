//! The detectors `north ledger flow detect` runs, in the order it runs them, and the name the ledger records each
//! under.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Detector {
    StalledPlan,
    SilentSignal,
    RepeatedRefusal,
    LowDisk,
    RecurringSequence,
    RepeatedCorrection,
    SlowTest,
    SlowCheck,
    CrateHeadroom,
    MachineFix,
    MachineRefusal,
    SkillReview,
    CodeHealthReview,
    FollowedSource,
    MechanismDiscovery,
}

impl Detector {
    pub const ALL: [Self; 15] = [Self::StalledPlan, Self::SilentSignal, Self::RepeatedRefusal, Self::LowDisk, Self::RecurringSequence, Self::RepeatedCorrection, Self::SlowTest, Self::SlowCheck, Self::CrateHeadroom, Self::MachineFix, Self::MachineRefusal, Self::SkillReview, Self::CodeHealthReview, Self::FollowedSource, Self::MechanismDiscovery];

    /// Whether the detector files each finding as an issue of its own rather than as a box on a standing owner: the
    /// daily mechanism-discovery turn, so no issue has to stay open to hold it.
    pub fn files_own_issue(self) -> bool {
        matches!(self, Self::MechanismDiscovery)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::StalledPlan => "stalled-plan",
            Self::SilentSignal => "silent-signal",
            Self::RepeatedRefusal => "repeated-refusal",
            Self::LowDisk => "low-disk",
            Self::RecurringSequence => "recurring-sequence",
            Self::RepeatedCorrection => "repeated-correction",
            Self::SlowTest => "slow-test",
            Self::SlowCheck => "slow-check",
            Self::CrateHeadroom => "crate-headroom",
            Self::MachineFix => "machine-fix",
            Self::MachineRefusal => "machine-refusal",
            Self::SkillReview => "skill-review",
            Self::CodeHealthReview => "code-health-review",
            Self::FollowedSource => "followed-source",
            Self::MechanismDiscovery => "mechanism-discovery",
        }
    }
}
