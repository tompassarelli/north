use super::affected::Scope;
use super::shared_build;

const CLIPPY: &str = "cargo clippy";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub clippy: String,
    pub prebuild: Option<String>,
    pub rest: Vec<String>,
}

impl Plan {
    pub fn first(&self) -> Vec<String> {
        std::iter::once(self.clippy.clone()).chain(self.prebuild.clone()).collect()
    }
}

pub fn plan(commands: &[String], listed: impl Fn(&str) -> bool) -> Option<Plan> {
    let clippy = commands.iter().find(|command| command.contains(CLIPPY) && listed(command))?;
    let rest: Vec<String> = commands.iter().filter(|command| *command != clippy).cloned().collect();
    Some(Plan { clippy: clippy.clone(), prebuild: shared_build::command(&rest), rest })
}

pub fn order(plan: &Plan, scope: Option<&Scope>) -> String {
    let covered = scope.map_or_else(String::new, |scope| format!(" ({})", scope.arguments()));
    let mut line = format!("safe-push: land check order: 1. clippy on the affected crates{covered}: {}", plan.clippy);
    if let Some(prebuild) = &plan.prebuild {
        line.push_str(&format!(", beside it the north build the head-north checks share: {prebuild}"));
    }
    if !plan.rest.is_empty() {
        line.push_str(&format!("; then {} land check(s): {}", plan.rest.len(), plan.rest.join("; ")));
    }
    line
}
