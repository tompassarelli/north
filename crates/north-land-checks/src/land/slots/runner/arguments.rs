use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use north_base::foundation::offload::{LANDER_SLOT_ENV, LOCAL_ENV, OFF_HOST_ENV};

use crate::land::slots::graph_selection::Batch;
use crate::land::slots::unit_reuse::RECORD_ENV;

#[derive(Clone, Default)]
pub(super) struct Arguments {
    pub(super) base: String,
    pub(super) head: String,
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
}

pub(super) fn parse(arguments: &[String]) -> Option<Arguments> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--portable" => parsed.portable = true,
            "--any" => parsed.any = true,
            "--after" => parsed.after = true,
            "--leased" => parsed.leased = true,
            "--replay" => parsed.replay = true,
            "--base" | "--head" | "--tree" | "--dry" | "--safe-push" | "--red-fix" | "--outcome" | "--destination" | "--event-log" | "--wait-event" | "--published" | "--runner-host" | "--command" | "--records" | "--since" | "--until" | "--lane" | "--ran" => {
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
                    _ => parsed.commands.push(value),
                }
            }
            _ => return None,
        }
    }
    (parsed.replay || !parsed.head.is_empty()).then_some(parsed)
}

impl Arguments {
    pub(super) fn tree(&self) -> &str {
        if self.tree.is_empty() { &self.head } else { &self.tree }
    }

    pub(super) fn pinned(&self) -> bool {
        self.tree() != self.head
    }
}

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
}

impl Environment {
    pub(super) fn local() -> Self {
        Self { capacity: String::new(), offload_host: String::new(), timeout: std::env::var("SAFE_PUSH_LAND_CHECK_TIMEOUT").ok().and_then(|value| value.parse().ok()), runner: None, off_host: false, local: false, lander_slot: false, record_reuse: false }
    }

    pub(super) fn load() -> Self {
        let text = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            capacity: text("SAFE_PUSH_LAND_CAPACITY"),
            offload_host: text("SAFE_PUSH_LAND_OFFLOAD_HOST"),
            timeout: text("SAFE_PUSH_LAND_CHECK_TIMEOUT").parse().ok(),
            runner: std::env::var("SAFE_PUSH_LAND_RUNNER").ok().filter(|runner| !runner.is_empty()),
            off_host: std::env::var(OFF_HOST_ENV).is_ok_and(|value| value == "1"),
            local: std::env::var(LOCAL_ENV).is_ok_and(|value| value == "1"),
            lander_slot: std::env::var(LANDER_SLOT_ENV).is_ok_and(|value| value == "1"),
            record_reuse: std::env::var(RECORD_ENV).is_ok_and(|value| value == "1"),
        }
    }
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
