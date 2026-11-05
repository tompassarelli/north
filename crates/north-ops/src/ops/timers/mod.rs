mod cross_project;
mod eta_calibration;
mod fleet_session;
mod gcroots;
mod git_maintenance;
mod incident_retro;
mod nexus_run;
mod runtime_update;
mod skill_prune;
mod skill_review;
mod supply_chain;
mod watch_review;
#[cfg(test)]
mod tests;

use std::env;
use std::path::PathBuf;
use std::process::{Command, Output};

use super::runtime_access::RuntimeAccess;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Timer {
    AgentRuntimeUpdate,
    CodexRuntimeGcroots,
    CrossProjectScan,
    EtaCalibration,
    GitMaintenanceNightly,
    IncidentRetro,
    JobWatch,
    ModelWatch,
    OutsideWatch,
    SkillPrune,
    SkillReviewQueue,
    SupplyChainScan,
    UsageWebRead,
    WatchReview,
}

impl Timer {
    pub const NAMES: &[(&str, Self)] = &[
        ("agent-runtime-update", Self::AgentRuntimeUpdate),
        ("codex-runtime-gcroots", Self::CodexRuntimeGcroots),
        ("cross-project-scan", Self::CrossProjectScan),
        ("eta-calibration", Self::EtaCalibration),
        ("git-maintenance-nightly", Self::GitMaintenanceNightly),
        ("incident-retro", Self::IncidentRetro),
        ("job-watch", Self::JobWatch),
        ("model-watch", Self::ModelWatch),
        ("outside-watch", Self::OutsideWatch),
        ("skill-prune", Self::SkillPrune),
        ("skill-review-queue", Self::SkillReviewQueue),
        ("supply-chain-scan", Self::SupplyChainScan),
        ("usage-web-read", Self::UsageWebRead),
        ("watch-review", Self::WatchReview),
    ];

    pub fn named(command: &str) -> Option<Self> {
        Self::NAMES.iter().find(|(name, _)| *name == command).map(|(_, timer)| *timer)
    }

    pub fn run(self, arguments: &[String], access: &RuntimeAccess) -> u8 {
        match self {
            Self::AgentRuntimeUpdate => runtime_update::run(arguments),
            Self::CodexRuntimeGcroots => gcroots::run(arguments),
            Self::CrossProjectScan => cross_project::run(arguments, access),
            Self::EtaCalibration => eta_calibration::run(arguments),
            Self::GitMaintenanceNightly => git_maintenance::run(arguments),
            Self::IncidentRetro => incident_retro::run(arguments),
            Self::JobWatch => crate::ops::job_watch::run(arguments),
            Self::ModelWatch => crate::ops::model_watch::run(arguments, access),
            Self::OutsideWatch => crate::ops::outside_watch::run(arguments),
            Self::SkillPrune => skill_prune::run(arguments),
            Self::SkillReviewQueue => skill_review::run(arguments),
            Self::SupplyChainScan => supply_chain::run(arguments),
            Self::UsageWebRead => crate::ops::usage_web_read::run(arguments),
            Self::WatchReview => watch_review::run(arguments),
        }
    }

}

pub use nexus_run::run as nexus_run;

fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap_or_default())
}

fn env_or(name: &str, fallback: impl FnOnce() -> PathBuf) -> PathBuf {
    env::var_os(name).filter(|value| !value.is_empty()).map_or_else(fallback, PathBuf::from)
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn date(format: &str) -> String {
    Command::new("date").arg(format).output().map(|output| text(&output).trim_end_matches('\n').to_owned()).unwrap_or_default()
}

fn child_failure(name: &str, code: Option<i32>, stderr: &[u8]) -> String {
    let code = code.unwrap_or(1);
    let stderr = String::from_utf8_lossy(stderr);
    match stderr.lines().map(str::trim).rfind(|line| !line.is_empty()) {
        Some(line) => format!("{name} exited {code}: {line}"),
        None => format!("{name} exited {code}"),
    }
}

fn north_program() -> std::ffi::OsString {
    env::var_os("NORTH_BIN").filter(|value| !value.is_empty()).unwrap_or_else(|| "north".into())
}

fn on_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .any(|dir| dir.join(name).metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0))
}
