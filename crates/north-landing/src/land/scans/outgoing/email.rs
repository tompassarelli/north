use std::path::Path;

use crate::landing::safe_push_state::refuse;
use crate::landing::step::{Step, git_quiet, git_read};

pub fn commit_email_scan(dir: &Path, revisions: &[String]) -> Step {
    let pushing: Vec<String> = ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .collect();
    match outgoing_email_refusal(dir, revisions, &pushing) {
        Some(message) => Err(refuse(message)),
        None => Ok(()),
    }
}

pub fn outgoing_email_refusal(dir: &Path, revisions: &[String], pushing: &[String]) -> Option<String> {
    let own = git_quiet(dir, &["config", "user.email"]).unwrap_or_default();
    if own.is_empty() {
        return Some(
            "safe-push: ✗ git config user.email is empty, so outgoing author and committer emails cannot be checked; set it before pushing; NOT pushed."
                .to_owned(),
        );
    }
    let mut allowed = vec![own.to_lowercase()];
    allowed.extend(pushing.iter().filter(|email| !email.is_empty()).map(|email| email.to_lowercase()));
    let extra = git_quiet(dir, &["config", "--get-all", "safe-push.allowEmail"]).unwrap_or_default();
    allowed.extend(extra.lines().filter(|email| !email.is_empty()).map(str::to_lowercase));
    let mut arguments = vec!["log", "--format=%H %ae %ce"];
    arguments.extend(revisions.iter().map(String::as_str));
    let Ok(log) = git_read(dir, &arguments) else {
        return Some("safe-push: cannot read the outgoing commit emails; NOT pushed.".to_owned());
    };
    for line in log.lines() {
        let mut fields = line.split_whitespace();
        let Some(sha) = fields.next() else { continue };
        for (role, email) in ["author", "committer"].into_iter().zip([fields.next(), fields.next()]) {
            let email = email.unwrap_or("");
            if allowed.contains(&email.to_lowercase()) {
                continue;
            }
            let short = git_quiet(dir, &["rev-parse", "--short", sha]).unwrap_or_default();
            return Some(format!(
                "safe-push: commit {short} {role} email {email} is not {}; re-author with git commit --amend --reset-author (or rebase --exec) before pushing; NOT pushed.",
                allowed.join(", ")
            ));
        }
    }
    None
}

#[cfg(test)]
#[path = "email_tests.rs"]
mod tests;
