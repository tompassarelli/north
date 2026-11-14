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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::path::Path;
    use std::process::{ExitStatus, Output};
    use std::time::Duration;

    use super::*;
    use crate::detectors::tick;
    use crate::flow::{Host, Places};

    struct Skills;

    impl Host for Skills {
        fn output(&self, argv: &[&str], _: Option<&Path>, _: Option<&str>, _: Duration) -> Option<Output> {
            let stdout = match argv {
                ["git", "log", "origin/main", ..] => "\u{1}1793750400\tTeach debugging the repro step\nharness/skills/debugging/SKILL.md\n\u{1}1793000000\tTeach workers the lease\nharness/skills/workers/SKILL.md\n",
                _ => return None,
            };
            Some(Output { status: ExitStatus::from_raw(0), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() })
        }
    }

    fn skill(id: &str) -> Skill {
        Skill { id: id.into(), reviewed: "2026-11-01".into(), ..Skill::default() }
    }

    #[test]
    fn a_skill_with_a_correction_since_its_review_files_one_box_and_an_unchanged_skill_none() {
        let corrected = Skill { corrections: vec!["2026-11-05T10:00:00Z".into()], ..skill("debugging") };
        let found = due(&Facts { skills: vec![corrected, skill("workers")] });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "skill-review:debugging:2026-11-01");
        assert!(found[0].owner.is_none() && found[0].detector.files_own_issue(), "a skill due for review is filed as an issue of its own");
        assert!(found[0].detail.contains("corrections classed to it were seen 1 time"));

        let root = tempfile::tempdir().unwrap();
        let places = Places { home: root.path().join("home"), state: root.path().join("state"), ledger: root.path().join("threads.db"), graph: root.path().join("graph.db") };
        for (id, written) in [("debugging", "2026-11-01"), ("workers", "2026-11-09")] {
            let directory = places.checkout("north").join("harness/skills").join(id);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("SKILL.md"), format!("---\nname: {id}\ngrounded: 2026-10-15\nwritten: {written}\n---\n# {id}\n")).unwrap();
        }
        let lines = tick(&places, &Skills, 1.0, false);
        let found: Vec<&String> = lines.iter().filter(|line| line.starts_with("detectors: skill-review skill-review:")).collect();
        assert_eq!(found, ["detectors: skill-review skill-review:debugging:2026-11-01: Review the debugging skill"]);
    }
}
