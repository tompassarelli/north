use std::fs;
use std::path::{Path, PathBuf};

use super::gate::{Budget, Verdicts};
use super::portable;

const SKILL_ROOTS: [&str; 2] = ["harness/skills", "agent-machinery/skills"];

fn skill_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("SKILL.md"))
        .filter(|path| path.is_file())
        .collect();
    found.sort();
    found
}

pub(super) fn tree_policy(tree: &Path) -> Verdicts {
    let mut budget = Budget::default();
    let mut errors: Vec<String> = Vec::new();
    let mut count = 0;
    let mut portable_count = 0;
    for root in SKILL_ROOTS {
        for skill in skill_files(&tree.join(root)) {
            count += 1;
            let directory = skill.parent().unwrap_or(tree);
            let unit = directory.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            match fs::read_to_string(&skill) {
                Ok(text) => {
                    budget.measure(&unit, &text);
                    if portable::declared(&text) {
                        portable_count += 1;
                        errors.extend(portable::boundary(tree, directory, &unit));
                    }
                }
                Err(error) => errors.push(format!("{}: {error}", skill.display())),
            }
        }
    }
    errors.append(&mut budget.errors);
    let passed = errors.is_empty() && count > 0;
    let text = if !errors.is_empty() {
        errors.join("\n")
    } else if count == 0 {
        format!("tree policy: no SKILL.md found under {} in {}", SKILL_ROOTS.join(" or "), tree.display())
    } else {
        format!("tree policy: {count} skills ({portable_count} portable) · {}", budget.summary())
    };
    Verdicts { text, passed }
}
