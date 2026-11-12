use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Role;
use crate::platform::session_host_files;

use super::Fleet;

/// The session whose launches may keep their children's gates aside, beside the eval units.
pub const EVALS_LEAD: &str = "evals-lead";
const EVAL_UNIT_PREFIX: &str = "eval-";

/// Whether the process whose `/proc/PID/cgroup` reads `cgroup` runs in an eval's own transient unit, which a fleet session's processes cannot leave.
pub fn in_eval_unit(cgroup: &str) -> bool {
    let leaf = cgroup.lines().last().and_then(|line| line.rsplit('/').next()).unwrap_or_default();
    leaf.starts_with(EVAL_UNIT_PREFIX) && (leaf.ends_with(".service") || leaf.ends_with(".scope"))
}

impl Fleet {
    /// Refuses a launch that asks for `--gates-aside` unless an eval makes it: the process runs in an `eval-*` unit, or it runs in the scope of the live `evals-lead` lead session, or it replays the entry of the launch queue named `name`, the launch being made, that was admitted with the flag. `cgroup` is the launcher's `/proc/self/cgroup`, a fact the launcher cannot change, never its environment or arguments.
    pub fn admit_gates_aside(&self, asked: bool, name: &str, caller: Option<&str>, queued: Option<&str>, cgroup: &str) -> NorthResult<()> {
        if !asked || in_eval_unit(cgroup) || self.queued_with_gates_aside(name, queued) || self.is_evals_lead(caller, cgroup) {
            return Ok(());
        }
        Err(NorthError::Refused(format!("--gates-aside is only for an eval: launch from an {EVAL_UNIT_PREFIX}* unit or from the {EVALS_LEAD} session, so no other session hides its children's gates from the owner")))
    }

    fn queued_with_gates_aside(&self, name: &str, queued: Option<&str>) -> bool {
        queued == Some(name) && self.queued_launches().is_ok_and(|launches| launches.iter().any(|launch| launch.name == name && launch.request.gates_aside))
    }

    fn is_evals_lead(&self, caller: Option<&str>, cgroup: &str) -> bool {
        let Some(caller) = caller.filter(|caller| *caller == EVALS_LEAD) else { return false };
        let scope = self.session_dir(caller).ok().and_then(|dir| session_host_files::scope(&dir));
        self.role_of(caller) == Some(Role::Lead) && scope.is_some_and(|unit| cgroup.lines().flat_map(|line| line.rsplit('/')).any(|leaf| leaf == unit))
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live};
    use super::*;

    const WORKER: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-aaaa.scope\n";
    const EVAL: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/eval-b-ladder.service\n";

    #[test]
    fn a_launch_with_gates_aside_from_an_ordinary_session_is_refused_and_one_from_an_eval_unit_or_the_evals_lead_goes_ahead() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let _worker = live(&laptop, "worker");
        let _lead = live(&laptop, EVALS_LEAD);
        let lead_dir = laptop.session_dir(EVALS_LEAD).unwrap();
        std::fs::write(session_host_files::scope_path(&lead_dir), "north-session-bbbb.scope").unwrap();
        let lead_cgroup = "0::/user.slice/agent.slice/north-session-bbbb.scope\n";

        let refusal = laptop.admit_gates_aside(true, "child", Some("worker"), None, WORKER).unwrap_err();
        assert!(matches!(refusal, NorthError::Refused(_)) && refusal.to_string().contains("--gates-aside"), "{refusal}");
        assert!(laptop.admit_gates_aside(true, "child", Some(EVALS_LEAD), None, WORKER).is_err(), "naming the lead from another session's scope is not the lead");
        assert!(laptop.admit_gates_aside(true, "child", None, None, WORKER).is_err());
        laptop.admit_gates_aside(false, "child", Some("worker"), None, WORKER).unwrap();
        laptop.admit_gates_aside(true, "child", None, None, EVAL).unwrap();
        laptop.admit_gates_aside(true, "child", Some(EVALS_LEAD), None, lead_cgroup).unwrap();
    }
}
