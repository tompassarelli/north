use super::{Detector, Finding};

/// One skill and what happened to it after its last review, which is the later of its `grounded:` and `written:` dates.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Skill {
    pub id: String,
    pub reviewed: String,
    pub changes: Vec<String>,
    pub corrections: Vec<String>,
    pub drift: Vec<String>,
}

#[derive(Default)]
pub struct Facts {
    pub skills: Vec<Skill>,
}

/// One box per skill and last review date: a skill whose files changed, whose correction class was seen again, whose
/// tool version moved or whose cited source changed since its review is read again, and an unchanged skill is not.
pub fn due(facts: &Facts) -> Vec<Finding> {
    facts
        .skills
        .iter()
        .filter(|skill| !(skill.changes.is_empty() && skill.corrections.is_empty() && skill.drift.is_empty()))
        .map(|skill| {
            let id = &skill.id;
            let mut signals = Vec::new();
            if !skill.changes.is_empty() {
                signals.push(format!("{} commit{} changed its files ({})", skill.changes.len(), if skill.changes.len() == 1 { "" } else { "s" }, skill.changes.join("; ")));
            }
            if !skill.corrections.is_empty() {
                signals.push(format!("Tom's corrections classed to it were seen {} time{} ({})", skill.corrections.len(), if skill.corrections.len() == 1 { "" } else { "s" }, skill.corrections.join(", ")));
            }
            signals.extend(skill.drift.iter().cloned());
            Finding {
                detector: Detector::SkillReview,
                key: format!("skill-review:{id}:{}", skill.reviewed),
                owner: None,
                title: format!("Review the {id} skill"),
                detail: format!("Since its last review on {}: {}. Recheck its facts against the real tools, versions and paths where it was grounded, and compress its prose and move knowledge to its one owner where it was written, without changing facts.", skill.reviewed, signals.join("; ")),
                done_when: format!("harness/skills/{id}/SKILL.md carries a `written:` (and, when facts were rechecked, a `grounded:`) date after {} and the review commit names which review it was.", skill.reviewed),
                instances: skill.changes.clone(),
            }
        })
        .collect()
}
