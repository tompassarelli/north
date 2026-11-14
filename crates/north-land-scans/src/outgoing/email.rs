use std::path::Path;

use crate::landing::step::refuse;
use crate::landing::step::{Step, git_quiet, git_read};

pub fn commit_email_scan(dir: &Path, revisions: &[String]) -> Step {
    match outgoing_email_refusal(dir, revisions) {
        Some(message) => Err(refuse(message)),
        None => Ok(()),
    }
}

const DEFAULT_LIST: &str = "*@users.noreply.github.com";

fn named_list(dir: &Path) -> Vec<String> {
    let committed = ["origin/main:.commit-emails", "HEAD:.commit-emails"]
        .into_iter()
        .map(|blob| git_quiet(dir, &["show", blob]).unwrap_or_default())
        .find(|entries| !entries.is_empty())
        .unwrap_or_default();
    let operator = git_quiet(dir, &["config", "--get-all", "safe-push.allowEmail"]).unwrap_or_default();
    let named: Vec<String> = committed.lines().chain(operator.lines()).map(str::trim).filter(|email| !email.is_empty()).map(str::to_lowercase).collect();
    if named.is_empty() { vec![DEFAULT_LIST.to_owned()] } else { named }
}

fn on_list(allowed: &[String], email: &str) -> bool {
    let email = email.to_lowercase();
    allowed.iter().any(|entry| match (entry.strip_prefix("*@"), email.rsplit_once('@')) {
        (Some(domain), Some((local, host))) => !local.is_empty() && host == domain,
        _ => *entry == email,
    })
}

pub fn outgoing_email_refusal(dir: &Path, revisions: &[String]) -> Option<String> {
    let allowed = named_list(dir);
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
            if on_list(&allowed, email) {
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
