use std::path::Path;

use crate::landing::safe_push_plan::{self, Arguments, Plan};

use super::candidate::git;
use super::lander::state_home;
use super::{enqueue, hold, queue, repositories, setting};

pub const PER_LANE: u8 = 3;

const USAGE: &str = "usage: north land route [--to BRANCH] [--direct] [--message FILE | --several-commits REASON] [--keep-lane]";

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub queue: String,
    pub hosts: Vec<String>,
    pub committed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Lander,
    PerLane,
    Refused(String),
}

impl Settings {
    pub fn read(repo: &Path, to: &str) -> Self {
        let remote_ref = format!("refs/remotes/origin/{to}");
        let blob = format!("{remote_ref}:.safe-push");
        let queue = git(repo, &["config", "--blob", &blob, "--get", "landing.queue"]).unwrap_or_default();
        let hosts = ["landing.lander-host", "landing.remote-hosts"]
            .iter()
            .flat_map(|key| setting::landing(repo, &remote_ref, key).split_whitespace().map(str::to_owned).collect::<Vec<_>>())
            .collect();
        let committed = git(repo, &["config", "--blob", &blob, "--get-regexp", r"^landing\.(lander-host|remote-hosts)$"]).is_ok_and(|found| !found.is_empty());
        Self { queue: if queue.is_empty() { "lander".to_owned() } else { queue }, hosts, committed }
    }
}

fn short(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}

pub fn decide(settings: &Settings, host: &str, to: &str, request: &Arguments, unserved: bool) -> Route {
    if settings.queue != "lander" {
        return Route::PerLane;
    }
    let named = settings.hosts.iter().any(|listed| short(listed) == short(host));
    if !settings.committed {
        return if named && !unserved && request.several_commits.is_none() && !request.direct { Route::Lander } else { Route::PerLane };
    }
    let host = short(host);
    if request.direct {
        return Route::Refused(format!("--direct is refused here: landing.queue is lander in .safe-push, so the lander lands every change to {to}; run safe-push --to {to} without --direct. NOT pushed."));
    }
    if !named {
        return Route::Refused(format!("{host} is neither landing.lander-host nor in landing.remote-hosts of .safe-push on {to}, so the lander cannot take this lane; add {host} to landing.remote-hosts. NOT pushed."));
    }
    Route::Lander
}

fn unserved(repo: &Path, to: &str, host: &str) -> bool {
    let lander_host = setting::landing(repo, &format!("refs/remotes/origin/{to}"), "landing.lander-host");
    if short(lander_host.trim()) != short(host) {
        return false;
    }
    let Ok(origin) = git(repo, &["remote", "get-url", "--push", "origin"]) else { return false };
    !hold::held(&queue::directory(&state_home(), &origin))
}

fn ticket_arguments(request: &Arguments) -> Vec<String> {
    let mut arguments = Vec::new();
    if let Some(message) = &request.message {
        arguments.extend(["--message".to_owned(), message.clone()]);
    }
    if let Some(reason) = &request.several_commits {
        arguments.extend(["--several-commits".to_owned(), reason.clone()]);
    }
    if request.keep_lane {
        arguments.push("--keep-lane".to_owned());
    }
    arguments
}

pub fn run(arguments: &[String]) -> u8 {
    let request = match safe_push_plan::arguments(arguments) {
        Ok(Plan::Help) => {
            println!("{USAGE}");
            return 0;
        }
        Ok(Plan::Push(request)) if request.tag.is_none() && !request.dry_run => request,
        Ok(Plan::Push(_)) => {
            eprintln!("{USAGE}");
            return 64;
        }
        Err(refusal) => {
            eprint!("{}", refusal.stderr());
            return refusal.exit_code();
        }
    };
    let repo = Path::new(".");
    let to = request.to.clone().unwrap_or_else(|| safe_push_plan::default_branch(repo));
    let settings = Settings::read(repo, &to);
    let host = repositories::local_host();
    let unserved = unserved(repo, &to, &host);
    match decide(&settings, &host, &to, &request, unserved) {
        Route::Lander => enqueue::run(&ticket_arguments(&request)),
        Route::PerLane => {
            if unserved && !settings.committed {
                eprintln!("safe-push: no lander serves this repository's queue on {}, so this lane lands per-lane", short(&host));
            }
            PER_LANE
        }
        Route::Refused(reason) => {
            eprintln!("safe-push: {reason}");
            1
        }
    }
}
