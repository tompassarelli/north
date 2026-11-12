use super::super::failure::{Failure, failed};
use north_base::foundation::carry::REMOTE_MARK;

const ROLE: &str = "NORTH_FLEET_ROLE";
const WORKER: &str = "worker";

/// A shipped or ordered runtime goes live for the host that shipped it or for the operator, whose shells carry neither a worker's role nor an offloaded job's mark.
pub(super) fn ships(verb: &str) -> Result<(), Failure> {
    ships_as(verb, |name| std::env::var(name).ok())
}

fn ships_as(verb: &str, said: impl Fn(&str) -> Option<String>) -> Result<(), Failure> {
    let caller = match (said(ROLE).as_deref() == Some(WORKER), said(REMOTE_MARK).is_some_and(|mark| !mark.is_empty())) {
        (true, _) => "a fleet worker session",
        (false, true) => "an offloaded job",
        (false, false) => return Ok(()),
    };
    Err(failed(1, format!("{verb} takes a runtime live for the host that shipped it or the operator, not for {caller}; runtime unchanged")))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Environment = &'static [(&'static str, &'static str)];

    fn said(environment: Environment) -> impl Fn(&str) -> Option<String> {
        move |name| environment.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
    }

    #[test]
    fn a_fleet_worker_session_or_an_offloaded_job_takes_no_shipped_or_ordered_runtime_live() {
        let refused = |environment: Environment| ships_as("north runtime update --sha --ordered", said(environment)).unwrap_err().message;
        assert!(refused(&[(ROLE, WORKER)]).ends_with("not for a fleet worker session; runtime unchanged"));
        assert!(refused(&[(REMOTE_MARK, "1")]).ends_with("not for an offloaded job; runtime unchanged"));
        let shipping: Environment = &[(ROLE, "proxy")];
        assert!(ships_as("north runtime update --sha", said(shipping)).is_ok() && ships_as("north runtime update --sha", said(&[])).is_ok(), "the shipping host's shell and the operator's carry neither mark");
    }
}
