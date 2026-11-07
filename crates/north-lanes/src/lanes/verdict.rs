#[derive(Clone, Copy, Debug)]
pub(super) struct Facts {
    pub(super) age: i64,
    pub(super) minimum_age: i64,
    pub(super) merged: bool,
    pub(super) equivalent: bool,
    pub(super) evil_merge: bool,
    pub(super) archive_age: Option<i64>,
    pub(super) protections: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Verdict {
    Keep,
    Retire,
    RetireEquivalent,
    ArchiveThenDelete,
}

pub(super) fn retirement_verdict(facts: &Facts) -> Verdict {
    if facts.protections != 0 || facts.age < facts.minimum_age {
        return Verdict::Keep;
    }
    if facts.merged {
        Verdict::Retire
    } else if facts.equivalent && !facts.evil_merge {
        Verdict::RetireEquivalent
    } else if facts.archive_age.is_some_and(|archive_age| facts.age >= archive_age) {
        Verdict::ArchiveThenDelete
    } else {
        Verdict::Keep
    }
}

pub(super) fn retires(merged: bool, protections: u32) -> bool {
    let facts = Facts { age: 0, minimum_age: 0, merged, equivalent: false, evil_merge: false, archive_age: None, protections };
    retirement_verdict(&facts) == Verdict::Retire
}
