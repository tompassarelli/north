use super::work::{Action, actions, all_out};
use super::world::{Name, Unix, World};

#[derive(Debug, Clone, PartialEq)]
pub struct AllOut {
    pub earliest_resume: Option<Unix>,
    pub announce: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Assessments {
    pub actions: Vec<(Name, Action)>,
    pub all_out: Option<AllOut>,
}

pub fn assess(world: &World, previous: &Assessments) -> Assessments {
    let all_out = all_out(world).map(|earliest_resume| AllOut { earliest_resume, announce: previous.all_out.is_none() });
    Assessments { actions: actions(world), all_out }
}
