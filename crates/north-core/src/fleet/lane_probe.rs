//! `stranded::Lanes` over git, here or over ssh on the machine the lanes live on.
use std::path::Path;
use std::time::Duration;

use crate::platform::process::run_within;

use super::model::Machine;
use super::stranded::{Checkout, Lane, Lanes, Worktree};

const LIMIT: Duration = Duration::from_secs(60);
const MARK: &str = "--lane";

pub struct Git;

impl Lanes for Git {
    fn checkout(&self, machine: Option<&Machine>, root: &Path) -> Option<Checkout> {
        let script = format!("cd {} && git fetch --quiet origin main && git worktree list --porcelain", quoted(root));
        Some(Checkout::parse(&run(machine, &script)?))
    }

    fn lanes(&self, machine: Option<&Machine>, trees: &[Worktree]) -> Vec<Lane> {
        if trees.is_empty() {
            return Vec::new();
        }
        let script: Vec<String> = trees
            .iter()
            .map(|tree| {
                format!(
                    "echo {MARK}; (cd {} && git rev-list --count origin/main..HEAD && git status --porcelain | wc -l && git merge-tree --write-tree origin/main HEAD | head -1 && git rev-parse 'origin/main^{{tree}}')",
                    quoted(&tree.path)
                )
            })
            .collect();
        let Some(said) = run(machine, &format!("{}; true", script.join("; "))) else { return Vec::new() };
        said.split(&format!("{MARK}\n")).skip(1).zip(trees).filter_map(|(block, tree)| standing(block, tree)).collect()
    }
}

fn standing(block: &str, tree: &Worktree) -> Option<Lane> {
    let [ahead, changed, merged, main]: [&str; 4] = block.lines().map(str::trim).collect::<Vec<_>>().try_into().ok()?;
    Some(Lane { path: tree.path.clone(), branch: tree.branch.clone(), ahead: ahead.parse().ok()?, dirty: changed.parse::<u32>().ok()? > 0, landed: merged == main })
}

fn run(machine: Option<&Machine>, script: &str) -> Option<String> {
    let output = match machine {
        None => run_within("sh", &["-c", script], LIMIT),
        Some(machine) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &machine.ssh, script], LIMIT),
    }?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}
