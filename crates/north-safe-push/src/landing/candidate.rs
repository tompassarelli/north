use north_base::foundation::step::Refused;

use crate::gates::plan::Gate;
use crate::gates::run::{Capacity, Gates, LandChecks, fetch_crates, sync_dependencies};
use crate::gates::target::Target;
use crate::report::terminal::cancelled_line;
use crate::report::timeline::Timeline;
use crate::session::cancel::watch_for_cancel;
use crate::session::output_log::claim;
use crate::shell::Shell;
use north_base::foundation::exit::NO_VERDICT;
use north_landing::land::lander::state_home;
use north_landing::land::landing_refusals;

const SCANS: [Gate; 10] = [
    Gate::Language,
    Gate::NativeRef,
    Gate::Secrets,
    Gate::Denylist,
    Gate::CommitEmail,
    Gate::DependencyGate,
    Gate::ToolRemoval,
    Gate::TestIntegrity,
    Gate::PolicySweep,
    Gate::StrategyReplay,
];

const BROKEN_BASE: &str = "main's check environment is broken: ";

#[derive(Debug, Eq, PartialEq)]
pub enum Verdict {
    Green(String),
    Refused(String),
    Transient(String),
    Deferred,
    RedMain(String),
}

impl Verdict {
    fn line(&self) -> String {
        let (class, detail) = match self {
            Self::Green(head) => ("green", head.as_str()),
            Self::Refused(detail) => ("refused", detail.as_str()),
            Self::Transient(detail) => ("transient", detail.as_str()),
            Self::Deferred => ("deferred", ""),
            Self::RedMain(base) => ("red-main", base.as_str()),
        };
        if detail.is_empty() { format!("safe-push: verdict {class}") } else { format!("safe-push: verdict {class} {detail}") }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct LandCheckResult {
    pub status: u8,
    pub skip: Option<String>,
    pub red_main: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Reached {
    DependenciesFailed,
    CrateFetchFailed { lock_changed: bool },
    ScansRefused,
    /// A scan that could not judge the lane, such as the private-kind judge timing out, exited no verdict.
    ScansNoVerdict,
    LandChecks(LandCheckResult),
}

pub fn classify(head: &str, reached: &Reached, log: &str) -> Verdict {
    let checks = match reached {
        Reached::DependenciesFailed => return Verdict::Transient("bun install failed".to_owned()),
        Reached::CrateFetchFailed { lock_changed: true } => return Verdict::Refused("cargo fetch --locked failed for the Cargo.lock this lane changed".to_owned()),
        Reached::CrateFetchFailed { lock_changed: false } => return Verdict::Transient("cargo fetch --locked failed".to_owned()),
        Reached::ScansRefused => return Verdict::Refused(last_actions_run(log).unwrap_or_default()),
        Reached::ScansNoVerdict => return Verdict::Transient("a scan reached no verdict, such as the private-kind judge timing out".to_owned()),
        Reached::LandChecks(checks) => checks,
    };
    if checks.status == 0 {
        return Verdict::Green(head.to_owned());
    }
    let skip = checks.skip.as_deref().unwrap_or("");
    if let Some(base) = &checks.red_main {
        Verdict::RedMain(base.clone())
    } else if checks.status == NO_VERDICT && skip.starts_with(BROKEN_BASE) {
        Verdict::Transient(skip.to_owned())
    } else if checks.status == NO_VERDICT && log.contains("DEFER") {
        Verdict::Deferred
    } else if checks.status == NO_VERDICT || skip.contains("bun install failed") {
        Verdict::Transient(if skip.is_empty() { format!("exit {NO_VERDICT}") } else { skip.to_owned() })
    } else {
        Verdict::Refused(last_actions_run(log).unwrap_or_default())
    }
}

fn last_actions_run(log: &str) -> Option<String> {
    const RUNS: &str = "/actions/runs/";
    log.split(|character: char| character.is_whitespace() || character == '(' || character == ')')
        .filter(|token| token.starts_with("http://") || token.starts_with("https://"))
        .filter_map(|token| {
            let at = token.rfind(RUNS)?;
            let digits = token[at + RUNS.len()..].bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0 && at > token.find("://")? + 3).then(|| token[..at + RUNS.len() + digits].to_owned())
        })
        .next_back()
}

enum Observed {
    Cancelled,
    Silent,
    Done(Facts),
}

struct Facts {
    reached: Reached,
    ran: bool,
}

pub fn run(base: &str, shell: &Shell) -> u8 {
    watch_for_cancel();
    let Ok(claims) = claim(shell) else { return 1 };
    let git = &*shell.git;
    let Some(root) = git.toplevel() else {
        println!("safe-push: not in a git repo");
        return 1;
    };
    let Some(origin_url) = git.push_url() else {
        println!("safe-push: origin has no push URL");
        return 1;
    };
    let Some(head) = git.resolve("HEAD^{commit}") else {
        println!("safe-push: cannot resolve HEAD to a commit");
        return 1;
    };
    let mut timeline = Timeline::start(shell);
    timeline.announce();
    let lanes = std::env::var(north_base::foundation::offload::LANES_ENV).ok().filter(|lanes| !lanes.is_empty()).unwrap_or_else(|| head.chars().take(12).collect());
    timeline.identify(&origin_url, &lanes, &head);
    let mut observed = Observed::Silent;
    let log = shell.processes.capture_output(&mut || observed = observe(shell, &timeline, &root, &origin_url, (base, &head), &claims.list));
    match observed {
        Observed::Cancelled => {
            println!("{}", cancelled_line());
            130
        }
        Observed::Silent => 1,
        Observed::Done(facts) => {
            let verdict = classify(&head, &facts.reached, &log);
            println!("{}", verdict.line());
            if matches!(verdict, Verdict::Green(_)) {
                if facts.ran {
                    shell.processes.exit_code("north", &["land", "clean", &root]);
                }
                return 0;
            }
            1
        }
    }
}

fn observe(shell: &Shell, timeline: &Timeline, root: &str, origin_url: &str, (base, head): (&str, &str), claimed: &str) -> Observed {
    let range = format!("{base}..{head}");
    let target = Target {
        root,
        origin_url,
        lane: head,
        branch: "",
        branch_ref: "",
        destination: "main",
        destination_ref: "refs/heads/main",
        destination_oid: "",
        base,
        head,
        range: &range,
    };
    if let Err(message) = sync_dependencies(std::path::Path::new(root)) {
        eprintln!("{message}");
        return Observed::Done(Facts { reached: Reached::DependenciesFailed, ran: false });
    }
    if let Err(message) = fetch_crates(std::path::Path::new(root)) {
        eprintln!("{message}");
        let lock_changed = crate::shell::git::quiet(std::path::Path::new(root), &["diff", "--name-only", &range, "--", "Cargo.lock"]).is_some_and(|changed| !changed.is_empty());
        return Observed::Done(Facts { reached: Reached::CrateFetchFailed { lock_changed }, ran: false });
    }
    let enforcing = landing_refusals::enforcing();
    for gate in SCANS {
        match target.scan(gate) {
            Ok(()) => {}
            Err(Refused(130 | 143)) => {
                println!("{}", cancelled_line());
                return Observed::Cancelled;
            }
            Err(Refused(69)) if gate == Gate::Language => return Observed::Silent,
            Err(Refused(NO_VERDICT)) => return Observed::Done(Facts { reached: Reached::ScansNoVerdict, ran: false }),
            Err(Refused(code)) if !enforcing && !gate.always_refuses() => {
                let lane = landing_refusals::Lane { at: "candidate", root, branch: "", head };
                lane.record(&state_home(), gate.label(), &format!("north land scan {} exited {code}", target.arguments(gate).unwrap_or_default().join(" ")));
            }
            Err(Refused(_)) => return Observed::Done(Facts { reached: Reached::ScansRefused, ran: false }),
        }
    }
    land_checks(shell, timeline, root, (base, head), claimed)
}

fn land_checks(shell: &Shell, timeline: &Timeline, root: &str, (base, head): (&str, &str), claimed: &str) -> Observed {
    let events = shell.settings.events_file();
    let wait = timeline.wait_line("land checks");
    let capacity = Capacity::default();
    let request = LandChecks { base, head, dry_run: false, destination: "main", event_log: &events, wait_event: &wait, capacity: &capacity, claimed };
    let checked = Gates::new(shell, std::path::Path::new(root)).land_checks(&request);
    let status = match checked.step {
        Ok(()) => 0,
        Err(Refused(130 | 143)) => {
            println!("{}", cancelled_line());
            return Observed::Cancelled;
        }
        Err(Refused(code)) => code,
    };
    for event in &checked.report.events {
        timeline.event(&event.stage, &event.status, &event.detail);
    }
    Observed::Done(Facts {
        reached: Reached::LandChecks(LandCheckResult { status, skip: checked.report.skip, red_main: checked.report.red_main }),
        ran: checked.report.ran,
    })
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod tests;
