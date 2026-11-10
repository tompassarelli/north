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
}

impl Detector {
    pub const ALL: [Self; 11] = [Self::StalledPlan, Self::SilentSignal, Self::RepeatedRefusal, Self::LowDisk, Self::RecurringSequence, Self::RepeatedCorrection, Self::SlowTest, Self::SlowCheck, Self::CrateHeadroom, Self::MachineFix, Self::MachineRefusal];

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
        }
    }
}
