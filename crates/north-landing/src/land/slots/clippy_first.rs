use super::affected::Scope;

const CLIPPY: &str = "cargo clippy";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub clippy: String,
    pub rest: Vec<String>,
}

pub fn plan(commands: &[String], listed: impl Fn(&str) -> bool) -> Option<Plan> {
    let clippy = commands.iter().find(|command| command.contains(CLIPPY) && listed(command))?;
    let rest = commands.iter().filter(|command| *command != clippy).cloned().collect();
    Some(Plan { clippy: clippy.clone(), rest })
}

pub fn order(plan: &Plan, scope: Option<&Scope>) -> String {
    let covered = scope.map_or_else(String::new, |scope| format!(" ({})", scope.arguments()));
    let mut line = format!("safe-push: land check order: 1. clippy on the affected crates{covered}: {}", plan.clippy);
    if !plan.rest.is_empty() {
        line.push_str(&format!("; then {} land check(s): {}", plan.rest.len(), plan.rest.join("; ")));
    }
    line
}

#[cfg(test)]
#[path = "clippy_first_tests.rs"]
mod tests;
