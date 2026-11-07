/// One way a program keeps state outside a typed lifecycle. A debt shape's new occurrence is landing debt; a
/// listed shape only appears in the repository's listing, because the same syntax also reads plain facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shape {
    MarkerStamp,
    PresenceFlag,
    UnitIntent,
    PidLiveness,
    PhaseSuffix,
    StatusString,
    LifecycleBools,
    MtimeRead,
    UnitRead,
    LinkRead,
    LockFlag,
    StateEntropy,
}

pub const SHAPES: [Shape; 12] = [
    Shape::MarkerStamp,
    Shape::PresenceFlag,
    Shape::UnitIntent,
    Shape::PidLiveness,
    Shape::PhaseSuffix,
    Shape::StatusString,
    Shape::LifecycleBools,
    Shape::MtimeRead,
    Shape::UnitRead,
    Shape::LinkRead,
    Shape::LockFlag,
    Shape::StateEntropy,
];

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Self::MarkerStamp => "marker-stamp",
            Self::PresenceFlag => "presence-flag",
            Self::UnitIntent => "unit-intent",
            Self::PidLiveness => "pid-liveness",
            Self::PhaseSuffix => "phase-suffix",
            Self::StatusString => "status-string",
            Self::LifecycleBools => "lifecycle-bools",
            Self::MtimeRead => "mtime-read",
            Self::UnitRead => "unit-read",
            Self::LinkRead => "link-read",
            Self::LockFlag => "lock-flag",
            Self::StateEntropy => "state-entropy",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        SHAPES.into_iter().find(|shape| shape.name() == name)
    }

    pub fn debt(self) -> bool {
        !matches!(self, Self::MtimeRead | Self::UnitRead | Self::LinkRead | Self::LockFlag | Self::StateEntropy)
    }

    pub(crate) fn why(self) -> &'static str {
        match self {
            Self::MarkerStamp => "a file stamped with the current time keeps a moment as state in the file system",
            Self::PresenceFlag => "a file this repository writes or removes is read here as a flag",
            Self::UnitIntent => "a unit stopped to hold an intent keeps that intent in systemd, where a deploy or reboot undoes it",
            Self::PidLiveness => "a pid's /proc entry or signal 0 answers for whatever process reuses the pid",
            Self::PhaseSuffix => "a name suffix is read as a phase",
            Self::StatusString => "a status is compared as a string outside the type that owns it",
            Self::LifecycleBools => "two or more lifecycle booleans encode a state machine whose illegal combinations compile",
            Self::MtimeRead => "a file's write time is read",
            Self::UnitRead => "a systemd unit's state is read",
            Self::LinkRead => "a symlink's target is read",
            Self::LockFlag => "a lock is taken on a file",
            Self::StateEntropy => "lifecycle booleans and optional fields together carry a state",
        }
    }
}
