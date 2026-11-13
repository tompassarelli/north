use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use north_base::foundation::offload::{LANDER_SLOT_ENV, LOCAL_ENV, OFF_HOST_ENV};
use north_base::platform::{harness, orchestration};
use toml_edit::DocumentMut;

use crate::land::slots::graph_selection::Batch;
use crate::land::slots::judged;
use crate::land::slots::select::Config;
use crate::land::slots::unit_reuse::RECORD_ENV;

#[derive(Clone, Default)]
pub(super) struct Arguments {
    pub(super) base: String,
    pub(super) head: String,
    pub(super) one_lane: bool,
    pub(super) line_within: Option<u64>,
    pub(super) twin_away: Option<Vec<String>>,
    pub(super) hold: Option<String>,
    pub(super) tree: String,
    pub(super) dry: bool,
    pub(super) safe_push: String,
    pub(super) red_fix: String,
    pub(super) outcome: Option<PathBuf>,
    pub(super) destination: Option<String>,
    pub(super) event_log: Option<PathBuf>,
    pub(super) wait_event: String,
    pub(super) portable: bool,
    pub(super) any: bool,
    pub(super) after: bool,
    pub(super) fresh: bool,
    pub(super) leased: bool,
    pub(super) replay: bool,
    pub(super) records: Option<PathBuf>,
    pub(super) since: u64,
    pub(super) until: u64,
    pub(super) published: String,
    pub(super) runner_host: String,
    pub(super) commands: Vec<String>,
    pub(super) span: String,
    pub(super) batch: Batch,
    pub(super) accept: Option<String>,
    pub(super) joins: bool,
}

pub(super) fn parse(arguments: &[String]) -> Option<Arguments> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--portable" => parsed.portable = true,
            "--any" => parsed.any = true,
            "--after" => parsed.after = true,
            "--fresh" => parsed.fresh = true,
            "--leased" => parsed.leased = true,
            "--replay" => parsed.replay = true,
            "--joins" => parsed.joins = true,
            "--base" | "--head" | "--tree" | "--dry" | "--safe-push" | "--red-fix" | "--outcome" | "--destination" | "--event-log" | "--wait-event" | "--published" | "--runner-host" | "--command" | "--records" | "--since" | "--until" | "--lane" | "--ran" | "--accept" => {
                let value = rest.next()?.clone();
                match flag.as_str() {
                    "--base" => parsed.base = value,
                    "--head" => parsed.head = value,
                    "--tree" => parsed.tree = value,
                    "--dry" => parsed.dry = value == "1",
                    "--safe-push" => parsed.safe_push = value,
                    "--red-fix" => parsed.red_fix = value,
                    "--outcome" => parsed.outcome = Some(PathBuf::from(value)),
                    "--destination" => parsed.destination = Some(value),
                    "--event-log" => parsed.event_log = Some(PathBuf::from(value)),
                    "--wait-event" => parsed.wait_event = value,
                    "--published" => parsed.published = value,
                    "--runner-host" => parsed.runner_host = value,
                    "--records" => parsed.records = Some(PathBuf::from(value)),
                    "--since" => parsed.since = value.parse().ok()?,
                    "--until" => parsed.until = value.parse().ok()?,
                    "--lane" => parsed.batch.lanes.push(value),
                    "--ran" => parsed.batch.ran.push(value),
                    "--accept" => parsed.accept = Some(value),
                    _ => parsed.commands.push(value),
                }
            }
            _ => return None,
        }
    }
    (parsed.replay || !parsed.head.is_empty()).then_some(parsed)
}

impl Arguments {
    pub(super) fn line_budget(&self) -> Option<u64> {
        self.line_within.or_else(|| std::env::var(north_base::foundation::carry::LINE_WITHIN).ok().and_then(|left| left.parse().ok()))
    }

    pub(super) fn tree(&self) -> &str {
        if self.tree.is_empty() { &self.head } else { &self.tree }
    }

    /// The lines that judge this run of `tree`: the landing's own for an after-landing run, else the lane's judged by `judged::config`.
    pub(super) fn judged(&self, root: &Path, tree: &str) -> Config {
        if self.after { Config::load(root, tree) } else { judged::config(root, &self.base, tree) }
    }

    pub(super) fn pinned(&self) -> bool {
        self.tree() != self.head
    }
}

const AT_LANDER_RANK: &str = "after_landing_at_lander_rank";

#[derive(Clone)]
pub(super) struct Environment {
    pub(super) capacity: String,
    pub(super) offload_host: String,
    pub(super) timeout: Option<u64>,
    pub(super) runner: Option<String>,
    pub(super) off_host: bool,
    pub(super) local: bool,
    pub(super) lander_slot: bool,
    pub(super) record_reuse: bool,
    pub(super) after_at_lander_rank: bool,
    pub(super) north: String,
}

impl Environment {
    pub(super) fn local(north: String) -> Self {
        Self { capacity: String::new(), offload_host: String::new(), timeout: std::env::var("SAFE_PUSH_LAND_CHECK_TIMEOUT").ok().and_then(|value| value.parse().ok()), runner: None, off_host: false, local: false, lander_slot: false, record_reuse: false, after_at_lander_rank: true, north }
    }

    pub(super) fn load(north: String) -> Self {
        let text = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            capacity: text("SAFE_PUSH_LAND_CAPACITY"),
            offload_host: text("SAFE_PUSH_LAND_OFFLOAD_HOST"),
            timeout: text("SAFE_PUSH_LAND_CHECK_TIMEOUT").parse().ok(),
            runner: std::env::var("SAFE_PUSH_LAND_RUNNER").ok().filter(|runner| !runner.is_empty()),
            off_host: off_host(std::env::var(OFF_HOST_ENV).is_ok_and(|value| value == "1"), orchestration::read().ok().as_ref(), &north_base::platform::host::name()),
            local: std::env::var(LOCAL_ENV).is_ok_and(|value| value == "1"),
            lander_slot: std::env::var(LANDER_SLOT_ENV).is_ok_and(|value| value == "1"),
            record_reuse: std::env::var(RECORD_ENV).is_ok_and(|value| value == "1"),
            after_at_lander_rank: harness::document(harness::DEFAULTS).get(AT_LANDER_RANK).and_then(|entry| entry.get("value")).and_then(|value| value.as_bool()).unwrap_or(true),
            north,
        }
    }
}

/// A host that `runs_no_checks` places every check on a build host the way an off-host set is, so no answering host leaves it waiting or without a verdict rather than running the check here.
pub(super) fn off_host(requested: bool, cfg: Option<&DocumentMut>, here: &str) -> bool {
    requested || cfg.is_some_and(|cfg| orchestration::runs_no_checks(cfg, here))
}

#[derive(Clone)]
pub(super) struct Outcome(pub(super) Option<PathBuf>);

impl Outcome {
    pub(super) fn record(&self, fields: &[&str]) {
        let Some(path) = &self.0 else { return };
        let line = fields.iter().map(|field| field.replace(['\t', '\n'], " ")).collect::<Vec<_>>().join("\t");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }
}
