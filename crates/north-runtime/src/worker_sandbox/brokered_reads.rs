use std::path::{Path, PathBuf};

use north_base::platform::session_scope;

use super::Context;
use super::lane_only::readable_in_lane;
use super::readable::readable;

/// The file a brokered `north home move` would read as its plan: its one operand that is not a flag, taken from the client's working directory, as the broker's verb would open it.
fn plan(arguments: &[String], directory: &Path) -> Option<PathBuf> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let ["home", "move", operands @ ..] = words.as_slice() else { return None };
    operands.iter().find(|word| !word.starts_with('-')).map(|word| directory.join(word))
}

/// Refuses a brokered verb that would read a file its client cannot: `north home move` opens its plan from outside the client's domain, so a plan path beyond what the client's domain lets it read (the logins, a lane-only client's other lanes and state) would hand back the first two tab fields of that file in its failure messages. Every other brokered verb reads nothing the client names: depbox, the capacity gate and the `--built`, `--lane` and `--program` commands run the client's program in a new domain at least as narrow, `lane unlock|stray|remove|gc` read lane names and enforce their own removal rule, and the `lane_grant` verbs read only the session's own lane.
pub(super) fn plan_readable(arguments: &[String], directory: &Path, environment: &[(String, String)], cgroup: Option<&str>) -> Result<(), String> {
    let Some(plan) = plan(arguments, directory) else { return Ok(()) };
    let real = plan.canonicalize().map_err(|error| format!("cannot read {}: {error}", plan.display()))?;
    let context = Context::from_request(&arguments.join(" "), directory, environment);
    let roots = if cgroup.is_none_or(session_scope::lane_only_cgroup) { readable_in_lane(&context, &[]) } else { readable(&context) };
    if roots.iter().any(|root| real.starts_with(root)) {
        Ok(())
    } else {
        Err(format!("{} is outside the files the caller can read", plan.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_move_plan_path_outside_the_clients_readable_set_is_refused() {
        let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        std::fs::create_dir_all(&outside_tmp).unwrap();
        let root = tempfile::Builder::new().prefix("north-plan-read").tempdir_in(outside_tmp).unwrap();
        let home = root.path().canonicalize().unwrap().join("home");
        let lane = home.join("code/north/worktrees/n1");
        let other = home.join("code/wisp/worktrees/n1");
        for directory in [&lane, &other, &home.join("code/north/main")] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::fs::write(lane.join("plan.tsv"), "a\tb\n").unwrap();
        std::fs::write(other.join("secret"), "hidden\tfield\tthird\n").unwrap();
        let environment = vec![("HOME".to_owned(), home.display().to_string())];
        let ask = |plan: &Path| plan_readable(&["home".to_owned(), "move".to_owned(), plan.display().to_string()], &lane, &environment, None);
        assert!(ask(&lane.join("plan.tsv")).is_ok(), "a plan inside the client's own lane is read");
        assert!(ask(&other.join("secret")).is_err(), "a lane-only client's plan beside it is refused");
    }
}
