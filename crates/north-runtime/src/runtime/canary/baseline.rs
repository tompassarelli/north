use super::super::layout::{self, Layout};
use super::super::lifecycle::{self, Canary, Canarying};
use super::probes::Probe;
use super::{Machine, Ran, run_probes, tail};

/// verify-north's last line (src/bin/verify_north/report.rs) names the step it stopped at.
const STOPPED_AT: &str = "verify-north FAILED at step ";
const EXITED: &str = " exited ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Broken {
    pub(super) probe: String,
    pub(super) step: Option<u32>,
}

impl Broken {
    pub(super) fn of(probe: &Probe, log: &str) -> Self {
        Self { probe: probe.name.clone(), step: stopped_at(log) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Passed,
    Failed(Option<u32>),
    Unknown,
}

pub(super) struct Baseline {
    pub(super) sha: String,
    failed: Vec<(String, Outcome)>,
}

impl Baseline {
    fn outcome(&self, probe: &str) -> Outcome {
        self.failed.iter().find(|(name, _)| name == probe).map_or(Outcome::Passed, |(_, outcome)| *outcome)
    }

    pub(super) fn beats(&self, broken: &[Broken]) -> bool {
        broken.iter().any(|broken| match self.outcome(&broken.probe) {
            Outcome::Passed => true,
            Outcome::Unknown => false,
            Outcome::Failed(theirs) => matches!((broken.step, theirs), (Some(ours), Some(theirs)) if ours < theirs),
        })
    }
}

pub(super) fn of(layout: &Layout, host: &str, failed_sha: &str, broken: &[Broken], machine: &impl Machine) -> Option<Baseline> {
    let sha = machine.rollback_target(failed_sha)?;
    let instance = format!("{host}/{sha}");
    match lifecycle::record_of::<Canarying>(layout, &instance).ok().flatten() {
        Some(record) if matches!(record.state, Canary::Passed | Canary::Kept) => Some(Baseline { failed: recorded(&record.cause), sha }),
        _ => Some(walk(layout, host, failed_sha, sha, broken, machine)),
    }
}

fn walk(layout: &Layout, host: &str, failed_sha: &str, sha: String, broken: &[Broken], machine: &impl Machine) -> Baseline {
    let instance = format!("{host}/{sha}");
    let (failed_short, short) = (layout::short(failed_sha), layout::short(&sha));
    if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, Canary::Judging, &format!("walking the core-loop probes on {short}, the runtime it would roll back to, which has no walk on record")) {
        println!("north runtime canary: {host}: {short} not walked as the rollback target of {failed_short}: {}", failure.message);
        return Baseline { failed: broken.iter().map(|broken| (broken.probe.clone(), Outcome::Unknown)).collect(), sha };
    }
    let (probes, ran) = run_probes(host, &sha, short, machine);
    let mut failed = Vec::new();
    let (mut failures, mut unrun) = (Vec::new(), Vec::new());
    for (probe, ran) in probes.iter().zip(&ran) {
        let outcome = match ran {
            Ran::Exited { code: 0, .. } => Outcome::Passed,
            Ran::Exited { code, log } => {
                failures.push(format!("probe `{}`{EXITED}{code}; log tail: {}", probe.name, tail(log)));
                Outcome::Failed(stopped_at(log))
            }
            Ran::Unrun(why) | Ran::HostFault(why) => {
                unrun.push(format!("probe `{}` not run: {why}", probe.name));
                Outcome::Unknown
            }
        };
        failed.push((probe.name.clone(), outcome));
    }
    failed.extend(broken.iter().filter(|broken| !probes.iter().any(|probe| probe.name == broken.probe)).map(|broken| (broken.probe.clone(), Outcome::Unknown)));
    let (state, cause) = match (failures.is_empty(), unrun.is_empty()) {
        (false, _) => (Canary::Kept, format!("walked as the rollback target of {failed_short}: {}", failures.iter().chain(&unrun).cloned().collect::<Vec<_>>().join("; "))),
        (true, true) => (Canary::Passed, format!("walked as the rollback target of {failed_short}: all {} core-loop probes exited 0", probes.len())),
        (true, false) => (Canary::Unjudged, format!("walked as the rollback target of {failed_short}: {}", unrun.join("; "))),
    };
    if let Err(failure) = lifecycle::move_to::<Canarying>(layout, &instance, state, &cause) {
        eprintln!("north runtime canary: {host}: {short}: {}", failure.message);
    }
    println!("north runtime canary: {host}: {short}, the rollback target of {failed_short}: {cause}");
    Baseline { sha, failed }
}

fn recorded(cause: &str) -> Vec<(String, Outcome)> {
    let probe = |segment: &str| segment.split_once('`').map(|(name, rest)| (name.to_owned(), if rest.starts_with(EXITED) { Outcome::Failed(stopped_at(rest)) } else { Outcome::Unknown }));
    cause.split("probe `").skip(1).filter_map(probe).collect()
}

fn stopped_at(log: &str) -> Option<u32> {
    let (_, rest) = log.rsplit_once(STOPPED_AT)?;
    rest.split(|character: char| !character.is_ascii_digit()).next()?.parse().ok()
}

pub(super) fn shared_key(host: &str, broken: &[Broken]) -> String {
    let named: Vec<String> = broken.iter().map(|broken| broken.step.map_or_else(|| broken.probe.clone(), |step| format!("{} step {step}", broken.probe))).collect();
    format!("runtime-canary-shared:{host}:{}", named.join(","))
}
