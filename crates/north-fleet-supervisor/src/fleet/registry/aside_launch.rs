use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Role};
use crate::platform::session_host_files;

use super::Fleet;

/// The session whose launches may keep their children's gates aside, beside the eval units.
pub const EVALS_LEAD: &str = "evals-lead";
const EVAL_UNIT_PREFIX: &str = "eval-";

/// A launch flag only an eval sets: its word, how a launch request shows it, and what it would let any other session do.
struct EvalFlag {
    word: &'static str,
    set: fn(&LaunchRequest) -> bool,
    misuse: &'static str,
}

const GATES_ASIDE: EvalFlag = EvalFlag { word: "--gates-aside", set: |request| request.gates_aside, misuse: "hides its children's gates from the owner" };
const CONTAINER_SHELL: EvalFlag = EvalFlag { word: "--container-shell", set: |request| request.container_shell, misuse: "starts a child whose shell runs on this host past the guards of its processes" };

/// Whether the process whose `/proc/PID/cgroup` reads `cgroup` runs in an eval's own transient unit, which a fleet session's processes cannot leave.
pub fn in_eval_unit(cgroup: &str) -> bool {
    let leaf = cgroup.lines().last().and_then(|line| line.rsplit('/').next()).unwrap_or_default();
    leaf.starts_with(EVAL_UNIT_PREFIX) && (leaf.ends_with(".service") || leaf.ends_with(".scope"))
}

/// Whether the process whose `/proc/PID/cgroup` reads `cgroup` runs in the login scope of the operator with `uid`, `session-N.scope` under that user's `user-UID.slice`, with no service or north unit among its parents.
fn in_login_scope(cgroup: &str, uid: u32) -> bool {
    let leaves: Vec<&str> = cgroup.lines().last().map(|line| line.split('/').collect()).unwrap_or_default();
    let login = leaves.last().and_then(|leaf| leaf.strip_prefix("session-")?.strip_suffix(".scope")).is_some_and(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()));
    let slice = format!("user-{uid}.slice");
    login && leaves.contains(&slice.as_str()) && leaves.iter().all(|leaf| !leaf.ends_with(".service") && !leaf.starts_with("north-"))
}

impl Fleet {
    /// Refuses a launch that asks for `--gates-aside` unless an eval makes it: the process runs in an `eval-*` unit, or it runs in the scope of the live `evals-lead` lead session, or it replays the entry of the launch queue named `name`, the launch being made, that was admitted with the flag. `cgroup` is the launcher's `/proc/self/cgroup`, a fact the launcher cannot change, never its environment or arguments.
    pub fn admit_gates_aside(&self, asked: bool, name: &str, caller: Option<&str>, queued: Option<&str>, cgroup: &str) -> NorthResult<()> {
        self.admit_eval_flag(asked, &GATES_ASIDE, name, caller, queued, cgroup)
    }

    /// Refuses a launch that asks for `--container-shell` unless an eval makes it, by the test of [`Self::admit_gates_aside`]: the flag stands the guards of this host's processes down for the session, so no session gives it to a child whose shell runs here.
    pub fn admit_container_shell(&self, asked: bool, name: &str, caller: Option<&str>, queued: Option<&str>, cgroup: &str) -> NorthResult<()> {
        self.admit_eval_flag(asked, &CONTAINER_SHELL, name, caller, queued, cgroup)
    }

    fn admit_eval_flag(&self, asked: bool, flag: &EvalFlag, name: &str, caller: Option<&str>, queued: Option<&str>, cgroup: &str) -> NorthResult<()> {
        if !asked || in_eval_unit(cgroup) || self.queued_with(flag.set, name, queued) || self.is_evals_lead(caller, cgroup) {
            return Ok(());
        }
        Err(NorthError::Refused(format!("{} is only for an eval: launch from an {EVAL_UNIT_PREFIX}* unit or from the {EVALS_LEAD} session, so no other session {}", flag.word, flag.misuse)))
    }

    /// Why this launch may not carry `--priority urgent`, or None when it may. Only the coordinator grants urgent, on positive proof: a process in the operator's login scope (the user slice of the uid that owns the fleet root, so another user's login never counts), in an eval unit, in the scope of the live proxy session, or replaying a queue entry admitted as urgent (a replay is judged by its entry, never by the tick that runs it); any other unit, session or not, is refused. `cgroup` is the launcher's `/proc/self/cgroup`, which a worker cannot change.
    pub fn urgent_denied(&self, asked: bool, name: &str, caller: Option<&str>, queued: Option<&str>, cgroup: &str) -> Option<String> {
        let replay = queued == Some(name);
        if !asked || self.queued_with(|request| request.urgent, name, queued) || (!replay && (self.in_operator_login(cgroup) || in_eval_unit(cgroup) || self.is_live_proxy(caller, cgroup))) {
            return None;
        }
        Some(format!("only the coordinator grants urgent priority, so {name} launches at normal priority"))
    }

    /// Whether the launcher may place work on a host's operator login: it runs outside every fleet session's scope, or in the scope of the lead or coordinator session it names. `cgroup` is the launcher's `/proc/self/cgroup`; a worker's processes cannot leave its session's scope, and naming a lead it does not run in proves nothing, so a confined caller is never the operator.
    pub fn operator_caller(&self, caller: Option<&str>, cgroup: &str) -> bool {
        let in_session = cgroup.lines().flat_map(|line| line.rsplit('/')).any(|leaf| leaf.starts_with(north_base::platform::session_scope::SESSION_UNIT_PREFIX));
        !in_session || caller.is_some_and(|caller| matches!(self.role_of(caller), Some(Role::Lead | Role::Proxy)) && self.in_scope_of(caller, cgroup))
    }

    fn in_operator_login(&self, cgroup: &str) -> bool {
        std::fs::metadata(&self.root).is_ok_and(|root| in_login_scope(cgroup, std::os::unix::fs::MetadataExt::uid(&root)))
    }

    fn is_live_proxy(&self, caller: Option<&str>, cgroup: &str) -> bool {
        caller.is_some_and(|caller| self.role_of(caller) == Some(Role::Proxy) && self.in_scope_of(caller, cgroup))
    }

    fn in_scope_of(&self, caller: &str, cgroup: &str) -> bool {
        let scope = self.session_dir(caller).ok().and_then(|dir| session_host_files::scope(&dir));
        scope.is_some_and(|unit| cgroup.lines().flat_map(|line| line.rsplit('/')).any(|leaf| leaf == unit))
    }

    fn queued_with(&self, set: fn(&LaunchRequest) -> bool, name: &str, queued: Option<&str>) -> bool {
        queued == Some(name) && self.queued_launches().is_ok_and(|launches| launches.iter().any(|launch| launch.name == name && set(&launch.request)))
    }

    fn is_evals_lead(&self, caller: Option<&str>, cgroup: &str) -> bool {
        let Some(caller) = caller.filter(|caller| *caller == EVALS_LEAD) else { return false };
        self.role_of(caller) == Some(Role::Lead) && self.in_scope_of(caller, cgroup)
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live};
    use super::*;

    const WORKER: &str = "0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-session-aaaa.scope\n";
    const EVAL: &str = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/eval-b-ladder.service\n";

    #[test]
    fn an_urgent_launch_the_caller_may_not_grant_says_so_and_never_fails_with_an_io_error() {
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
        let refusal = laptop.admit_container_shell(true, "child", Some("worker"), None, WORKER).unwrap_err();
        assert!(matches!(refusal, NorthError::Refused(_)) && refusal.to_string().contains("--container-shell"), "{refusal}");
        laptop.admit_container_shell(true, "child", None, None, EVAL).unwrap();
        let said = laptop.urgent_denied(true, "child", Some("worker"), None, WORKER).unwrap();
        assert!(said.contains("only the coordinator grants urgent") && said.contains("normal priority"), "{said}");
        assert!(laptop.urgent_denied(true, "child", None, None, WORKER).is_some());
        assert!(laptop.urgent_denied(false, "child", Some("worker"), None, WORKER).is_none());
        assert!(laptop.urgent_denied(true, "child", None, None, EVAL).is_none());
        let operator = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(home.path()).unwrap());
        assert!(laptop.urgent_denied(true, "child", None, None, &format!("0::/user.slice/user-{operator}.slice/session-3.scope\n")).is_none());
        assert!(laptop.urgent_denied(true, "child", None, None, &format!("0::/user.slice/user-{}.slice/session-3.scope\n", operator + 1)).is_some(), "an agent-uid login is not the operator's");
        for unit in ["0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-land@main.service\n", "0::/user.slice/user-1000.slice/user@1000.service/app.slice/north-worker-outside-1.service\n"] {
            assert!(laptop.urgent_denied(true, "child", None, None, unit).is_some(), "{unit}");
        }
    }
}
