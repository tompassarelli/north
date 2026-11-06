use std::path::Path;

use north_base::foundation::step::{Refused, Step};

use crate::report::timeline::Timeline;
use crate::request::{self, Arguments, Plan};
use crate::session::cancel::watch_for_cancel;
use crate::session::output_log::claim;
use crate::shell::Shell;
use crate::shell::git::default_branch;

use north_landing::land::candidate::git;
use north_landing::land::lander::state_home;
use north_landing::land::{enqueue, hold, queue, repositories, setting};

const USAGE: &str = "usage: north land route [--to BRANCH] [--message FILE | --several-commits REASON] [--keep-lane]";

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub queue: String,
    pub hosts: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Lander,
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
        Self { queue: if queue.is_empty() { "lander".to_owned() } else { queue }, hosts }
    }
}

fn short(host: &str) -> &str {
    host.split('.').next().unwrap_or(host)
}

fn unserved(repo: &Path, to: &str, host: &str) -> bool {
    let lander_host = setting::landing(repo, &format!("refs/remotes/origin/{to}"), "landing.lander-host");
    if short(lander_host.trim()) != short(host) {
        return false;
    }
    let Ok(origin) = git(repo, &["remote", "get-url", "--push", "origin"]) else { return false };
    !hold::held(&queue::directory(&state_home(), &origin))
}

pub fn decide(settings: &Settings, host: &str, to: &str, request: &Arguments) -> Route {
    let host = short(host);
    if settings.queue != "lander" {
        return Route::Refused(format!("landing.queue = {} in .safe-push is no longer served: the lander lands every change to {to}; remove landing.queue. NOT pushed.", settings.queue));
    }
    if request.direct {
        return Route::Refused(format!("--direct is refused: the lander lands every change to {to}; run safe-push --to {to} without --direct. NOT pushed."));
    }
    if !settings.hosts.iter().any(|listed| short(listed) == host) {
        return Route::Refused(format!("{host} is neither landing.lander-host nor in landing.remote-hosts for {to}, so the lander cannot take this lane; add {host} to landing.remote-hosts in .safe-push or this host's git config. NOT pushed."));
    }
    Route::Lander
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
    let request = match request::arguments(arguments) {
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
    let to = request.to.clone().unwrap_or_else(|| default_branch(repo));
    let settings = Settings::read(repo, &to);
    let host = repositories::local_host();
    match decide(&settings, &host, &to, &request) {
        Route::Lander => {
            if unserved(repo, &to, &host) {
                eprintln!("safe-push: no lander serves this repository's queue on {} yet; the lane waits in the queue until north land serve runs", short(&host));
            }
            enqueue::run(&ticket_arguments(&request))
        }
        Route::Refused(reason) => {
            eprintln!("safe-push: {reason}");
            1
        }
    }
}

pub fn land(arguments: &[String], to: &str, shell: &Shell) -> u8 {
    watch_for_cancel();
    let timeline = Timeline::start(shell);
    let code = match claim(shell) {
        Ok(_claims) => {
            timeline.announce();
            match fetch_destination(shell, to) {
                Ok(()) => run(arguments),
                Err(Refused(code)) => code,
            }
        }
        Err(Refused(code)) => code,
    };
    match code {
        _ if code == 0 || shell.settings.raw_exit => code,
        130 | 143 => {
            println!("safe-push: cancelled");
            130
        }
        _ => {
            timeline.event("exit", "refused", "safe-push did not land");
            println!("safe-push: refused safe-push did not land");
            1
        }
    }
}

fn fetch_destination(shell: &Shell, to: &str) -> Step {
    let reference = format!("refs/heads/{to}");
    if shell.origin.fetch(&format!("+{reference}:refs/remotes/origin/{to}")).is_ok() {
        return Ok(());
    }
    match shell.origin.listing("origin", &[&reference]) {
        Ok(listing) if listing.trim().is_empty() => Ok(()),
        _ => {
            eprintln!("safe-push: could not fetch origin/{to}; NOT pushed.");
            Err(Refused(1))
        }
    }
}

pub fn ticket(arguments: &[String]) -> u8 {
    enqueue::run(arguments)
}
