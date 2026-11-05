use std::collections::BTreeMap;

use super::spend::{Scope, Spend, spend, trips};
use super::work::{Action, actions, all_out};
use super::world::{Name, Trip, Unix, World};

#[derive(Debug, Clone, PartialEq)]
pub struct AllOut {
    pub earliest_resume: Option<Unix>,
    pub announce: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Assessments {
    pub spend: BTreeMap<Scope, Spend>,
    pub actions: Vec<(Name, Action)>,
    pub all_out: Option<AllOut>,
}

impl Assessments {
    pub fn trips(&self) -> Vec<Trip> {
        trips(&self.spend)
    }
}

pub fn assess(world: &World, previous: &Assessments) -> Assessments {
    let all_out = all_out(world).map(|earliest_resume| AllOut { earliest_resume, announce: previous.all_out.is_none() });
    Assessments { spend: spend(world, &previous.spend), actions: actions(world), all_out }
}
