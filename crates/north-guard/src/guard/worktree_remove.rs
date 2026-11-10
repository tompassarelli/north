use super::git_invocation::{bash_command, invocation, invocations, words};
use crate::hook_input::{HookInput, Verdict};
use north_base::platform::land_queue;
use north_base::platform::records_mirror::state_home;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static WORKTREE: LazyLock<Regex> = LazyLock::new(|| invocation("worktree", true));

pub fn evaluate(input: &HookInput) -> Verdict {
    decide(input, &land_queue::queues(&state_home()), &land_queue::local_host())
}

pub(super) fn decide(input: &HookInput, queues: &Path, host: &str) -> Verdict {
    let Some(command) = bash_command(input) else {
        return Verdict::Allow;
    };
    let cwd = PathBuf::from(input.cwd().unwrap_or_default());
    for (call, arguments) in invocations(&command, &WORKTREE) {
        let arguments: Vec<&str> = words(arguments).collect();
        if arguments.first() != Some(&"remove") {
            continue;
        }
        let directory = directory_options(call).into_iter().fold(cwd.clone(), |directory, option| directory.join(home(option)));
        for lane in arguments[1..].iter().filter(|word| !word.starts_with('-')).map(|word| directory.join(home(word))) {
            if let Some(ticket) = land_queue::waiting_on(queues, host, &lane).first() {
                return Verdict::Deny(format!(
                    "No. Land ticket {} still waits to land from {}, and the lander fetches it from that worktree. Leave it: the lander or north lane gc removes it once every ticket for it is decided.",
                    ticket.display(),
                    lane.display()
                ));
            }
            if lane.parent().and_then(Path::file_name).is_some_and(|name| name == "worktrees") {
                return Verdict::Deny(format!("No. A worker sandbox cannot delete a directory in worktrees/, so `git worktree remove` there unregisters {0} and leaves its files behind. Run `north lane remove {0}` instead: it removes the lane from outside the sandbox when it is clean and holds no unlanded work.", lane.display()));
            }
        }
    }
    Verdict::Allow
}

pub(super) fn directory_options(call: &str) -> Vec<&str> {
    let words: Vec<&str> = words(call).collect();
    words.windows(2).filter(|pair| pair[0] == "-C").map(|pair| pair[1]).collect()
}

pub(super) fn home(word: &str) -> PathBuf {
    match word.strip_prefix("~/").or_else(|| word.strip_prefix("$HOME/")) {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(word),
    }
}
