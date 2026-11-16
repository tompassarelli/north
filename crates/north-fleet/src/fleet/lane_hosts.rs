//! The lanes a launch's brief names, by their directory in a `Files:` path or by the session its `Follows:` line names, and
//! the hosts that hold them: a worker started anywhere else finds the lane missing only after it starts (north#1625).
use std::path::Path;

use north_foundation::error::{NorthError, NorthResult};

use super::lane_probe::{quoted, run};
use super::model::Machine;
use super::overlap::declared_files;

const MARK: &str = "/worktrees/";

/// The lane directories the brief names: those its `Files:` paths sit under, as `/home/tom/code/north/worktrees/rename-coordinator`,
/// and the lane of the session its `Follows:` line names, under the `worktrees/` beside the launch directory `cwd`.
pub fn named(brief: &str, cwd: &Path) -> Vec<String> {
    let files = declared_files(brief).into_iter().filter_map(|path| {
        let at = path.find(MARK)? + MARK.len();
        let lane = path[at..].split('/').next().filter(|lane| !lane.is_empty())?;
        Some(path[..at + lane.len()].to_owned())
    });
    let follows = brief.lines().filter_map(|line| line.strip_prefix("Follows:")?.split_whitespace().next()).filter_map(|slug| {
        let container = cwd.parent()?;
        Some(container.join("worktrees").join(slug).to_string_lossy().into_owned())
    });
    let mut lanes: Vec<String> = Vec::new();
    for lane in files.chain(follows) {
        if !lanes.contains(&lane) {
            lanes.push(lane);
        }
    }
    lanes
}

/// Why a launch on `host` is refused: it names a lane that `holders` finds on other hosts and not on `host`.
pub fn refusal(lanes: &[String], host: &str, holders: &dyn Fn(&str) -> Vec<String>) -> Option<String> {
    lanes.iter().find_map(|lane| {
        let held = holders(lane);
        (!held.is_empty() && !held.iter().any(|name| name == host)).then(|| format!("the brief names the lane {lane}, which only {} holds; launch it there, not on {host}", held.join(", ")))
    })
}

/// The hosts among this one (`here`) and `target`, the machine a launch starts on, that hold the lane directory `lane`.
pub fn holders(lane: &str, here: &str, target: Option<&Machine>) -> Vec<String> {
    let script = format!("test -d {}", quoted(Path::new(lane)));
    let mut held = Vec::new();
    if run(None, &script).is_some() {
        held.push(here.to_owned());
    }
    if let Some(machine) = target.filter(|machine| machine.name != here)
        && run(Some(machine), &script).is_some()
    {
        held.push(machine.name.clone());
    }
    held
}

/// Refuses a fresh launch on `machine` (this host when `None`) whose brief `prompt`, started in `cwd`, names a lane only another host holds.
pub fn check(prompt: &str, cwd: &Path, machine: Option<&Machine>) -> NorthResult<()> {
    let here = north_base::platform::host::name();
    let on = machine.map_or(here.as_str(), |machine| machine.name.as_str());
    refusal(&named(prompt, cwd), on, &|lane| holders(lane, &here, machine)).map_or(Ok(()), |why| Err(NorthError::Refused(why)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_naming_an_existing_lane_is_placed_on_its_host() {
        let cwd = Path::new("/home/tom/code/north/main");
        let brief = "Item: north#1484\nFollows: rename-coordinator (worker-sonnet)\nFiles: /home/tom/code/north/worktrees/rename-coordinator/src/lib.rs, /home/tom/code/north/worktrees/rename-coordinator/README.md.";
        let lane = "/home/tom/code/north/worktrees/rename-coordinator";
        let lanes = named(brief, cwd);
        assert_eq!(lanes, [lane]);
        assert_eq!(named("Follows: rename-coordinator (worker-sonnet)", cwd), [lane]);
        let held = |path: &str| if path == lane { vec!["gex45".to_owned()] } else { Vec::new() };
        assert_eq!(refusal(&lanes, "gex45", &held), None);
        let refused = refusal(&lanes, "cpx62b", &held).unwrap();
        assert!(refused.contains("only gex45 holds") && refused.contains("not on cpx62b"), "{refused}");
        assert_eq!(refusal(&lanes, "cpx62b", &|_| Vec::new()), None, "a lane no host holds is a new lane");
    }
}
