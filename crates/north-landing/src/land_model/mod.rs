pub mod buckets;
pub mod features;
pub mod replay;

pub const PRECHECK_ERA_START: i64 = 1_793_845_002;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Era {
    Batch,
    Precheck,
}

impl Era {
    pub fn at(decided: i64) -> Era {
        if decided < PRECHECK_ERA_START { Era::Batch } else { Era::Precheck }
    }

    pub fn name(self) -> &'static str {
        match self {
            Era::Batch => "batch",
            Era::Precheck => "precheck",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Landed,
    Conflict,
    Failed,
    Infra,
    Replaced,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub branch: String,
    pub era: Era,
    pub class: Class,
    pub queued: i64,
    pub decided: i64,
    pub shared_files: u32,
    pub resubmit: bool,
    pub lander_paths: bool,
    pub history_refused: bool,
    pub docs_only: bool,
}

#[cfg(test)]
#[path = "buckets_tests.rs"]
mod buckets_tests;
