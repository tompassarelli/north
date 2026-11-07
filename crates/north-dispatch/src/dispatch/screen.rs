use super::issues::{self, Issue};
use super::plan::Launch;
use super::survey::Survey;
use super::{brief, tier};

pub(super) fn admitted(survey: &Survey, issue: &Issue, world: &issues::World) -> Result<(), String> {
    if let Some(why) = survey.projects.paused(&issue.repo, survey.at) {
        return Err(why);
    }
    if let Some(topic) = crate::platform::frozen_topics::matching(&survey.frozen, &issue.repo, &issue.title) {
        return Err(topic.refusal());
    }
    issues::readiness(issue, world)
}

/// The route and brief a ready box launches with, or why none may be written.
pub(super) fn written(survey: &Survey, issue: &Issue, launched_by: &str) -> Result<(tier::Route, String), String> {
    let route = tier::route(issue, &survey.history, &survey.ledger)?;
    let grounds = survey.grounds.grounds(&issue.reference().to_string(), issue.created, &issue.body, &route).carrying(issue.stop.climb.as_ref().map(|climb| climb.report.as_str()).or(issue.stop.relay.as_ref().map(|relay| relay.report.as_str())));
    let brief = brief::brief(issue, launched_by, &route, &grounds);
    match crate::router::brief::class_refusal(&brief).or_else(|| crate::router::brief::grounding_refusal(&brief)).or_else(|| crate::platform::signin::brief_refusal(&brief)) {
        Some(why) => Err(why),
        None => Ok((route, brief)),
    }
}

/// Tag a local launch that carries no stamp with the context arm drawn for its box.
pub(super) fn arm(survey: &Survey, launch: &mut Launch) {
    if launch.cloud || launch.route.stamp().is_some() {
        return;
    }
    let evidence = survey.ledger.get(&launch.route.category).cloned().unwrap_or_default();
    let (arm, drawn) = tier::context_arm(&launch.item.to_string(), &launch.route.category, &survey.history, &evidence);
    launch.brief = brief::armed(&launch.brief, &arm);
    launch.route.why = format!("{}, context arm {arm} {drawn}", launch.route.why);
}
