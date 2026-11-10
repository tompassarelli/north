//! The path policy of `north --worker-sandbox`: what a confined command may read, execute and remove beneath. `north-runtime` applies it with Landlock and runs the command; this module holds the rules so a test can confine a child without the runtime's CLI.
pub mod except;
pub mod executable;
pub mod lane_only;
pub mod readable;
pub mod removable;

use std::path::PathBuf;

use landlock::{AccessFs, BitFlags, CompatLevel, Compatible, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules};

pub use executable::{build_outputs, executable};
pub use readable::readable;
pub use removable::{Context, removable};

fn removal() -> BitFlags<AccessFs> {
    AccessFs::RemoveDir | AccessFs::RemoveFile | AccessFs::Refer
}

/// Restricts this process, and everything it starts, to reading only files beneath `readable`, given `executable` to executing only files beneath it, and given `roots` to removing and renaming away only beneath them. Refer is handled either way, so a file cannot be linked or moved to where it would become readable or executable.
pub fn confine(roots: Option<&[PathBuf]>, readable: &[PathBuf], executable: Option<&[PathBuf]>) -> Result<RulesetStatus, String> {
    let handled = if roots.is_some() { removal() } else { BitFlags::from(AccessFs::Refer) };
    let handled = if executable.is_some() { handled | AccessFs::Execute } else { handled };
    let ruleset = landlock::Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(handled | AccessFs::ReadFile).map_err(|error| error.to_string())?;
    let ruleset = ruleset.create().map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(["/"], AccessFs::Refer)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(roots.unwrap_or_default().iter().filter(|root| root.is_dir()), removal())).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(readable, AccessFs::ReadFile)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(executable.unwrap_or_default(), AccessFs::Execute)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.set_compatibility(CompatLevel::BestEffort).log_new_exec(true).map_err(|error| error.to_string())?;
    ruleset.restrict_self().map(|status| status.ruleset).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
