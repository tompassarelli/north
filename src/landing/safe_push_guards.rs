use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::denylist;
use super::safe_push_state::{Refused, Step, cancelled, exit_code, git_quiet, git_read, refuse};

const NATIVE_REFS: [&str; 4] = ["life#", "accounts#", "travel#", "ops#"];

pub(crate) fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            let candidate = dir.join(program);
            candidate.is_file() && std::fs::metadata(&candidate).is_ok_and(|meta| {
                std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0
            })
        })
    })
}

pub(crate) fn language_gate(root: &Path, base: &str, head: &str) -> Step {
    let base = if base.is_empty() { git_read(root, &["hash-object", "-t", "tree", "/dev/null"])? } else { base.to_owned() };
    if crate::language_gate::check(root, &base, head) {
        return Ok(());
    }
    cancelled()?;
    eprintln!("safe-push: language gate refused the landing; NOT pushed.");
    Err(Refused(1))
}

pub(crate) fn names_native_item(text: &str) -> bool {
    text.lines().any(|line| {
        NATIVE_REFS.iter().any(|prefix| {
            line.match_indices(prefix).any(|(at, _)| {
                let bounded = line[..at].chars().next_back().is_none_or(|before| !(before.is_alphanumeric() || before == '_'));
                bounded && line[at + prefix.len()..].starts_with(|next: char| next.is_ascii_digit())
            })
        })
    })
}

pub(crate) fn native_ref_scan(dir: &Path, names: &[&str], revisions: &[String]) -> Step {
    let mut arguments = vec!["log", "--format=%B"];
    arguments.extend(revisions.iter().map(String::as_str));
    let Some(messages) = git_quiet(dir, &arguments) else {
        return Err(refuse("safe-push: cannot read the outgoing commit messages for the native-ref scan; NOT pushed."));
    };
    let text = format!("{}\n{messages}", names.join("\n"));
    if names_native_item(&text) {
        return Err(refuse(
            "safe-push: ✗ a branch name or commit message names a native NorthThread item (life/accounts/travel/ops#N); those stay in threads.db — reword without the ref; NOT pushed.",
        ));
    }
    Ok(())
}

pub(crate) fn history_secret_scan(dir: &Path, revisions: &[String]) -> Step {
    let log_opts = format!("--log-opts={}", revisions.join(" "));
    let clean = Command::new("gitleaks")
        .current_dir(dir)
        .args(["detect", "--no-banner", "--redact", &log_opts])
        .status()
        .is_ok_and(|status| status.success());
    if clean { Ok(()) } else { Err(refuse("safe-push: ✗ SECRET DETECTED in tag commit history — fix it; NOT pushed.")) }
}

pub(crate) fn annotation_secret_scan(dir: &Path, annotation: &[u8]) -> Step {
    let scratch = tempfile::tempdir().map_err(|_| Refused(1))?;
    std::fs::write(scratch.path().join("tag-object.txt"), annotation).map_err(|_| Refused(1))?;
    let source = scratch.path().to_string_lossy();
    let quiet = |args: &[&str]| {
        Command::new("gitleaks")
            .current_dir(dir)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    // gitleaks CLI drift: 8.19+ uses `dir`, older uses `detect --no-git --source`.
    if quiet(&["dir", &source, "--no-banner", "--redact"])
        || quiet(&["detect", "--no-git", "--source", &source, "--no-banner", "--redact"])
    {
        return Ok(());
    }
    Err(refuse("safe-push: ✗ SECRET DETECTED in tag annotation — fix it; NOT pushed."))
}

pub(crate) fn commit_email_scan(dir: &Path, revisions: &[String]) -> Step {
    let pushing: Vec<String> = ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .collect();
    match outgoing_email_refusal(dir, revisions, &pushing) {
        Some(message) => Err(refuse(message)),
        None => Ok(()),
    }
}

pub(crate) fn outgoing_email_refusal(dir: &Path, revisions: &[String], pushing: &[String]) -> Option<String> {
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

pub(crate) struct Denylist {
    pub(crate) south: PathBuf,
    pub(crate) config: Option<PathBuf>,
}

impl Denylist {
    pub(crate) fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let south = std::env::var_os("SAFE_PUSH_SOUTH").filter(|value| !value.is_empty()).map_or_else(|| home.join("code/south"), PathBuf::from);
        let config = std::env::var_os("SAFE_PUSH_DENYLIST").filter(|value| !value.is_empty()).map(PathBuf::from);
        Self { south, config }
    }

    pub(crate) fn scan(&self, root: &Path, origin_url: &str, base: &str, head: &str, tag: &str) -> Step {
        let configured = self.config.as_ref().is_some_and(|config| config.is_file());
        if !configured && !self.south.is_dir() {
            let named = self.config.as_ref().map_or_else(|| format!("{} origin/main", self.south.display()), |config| config.display().to_string());
            println!(
                "safe-push: ⚠ no deny-list at {named} and no south checkout at {}, so private values were not scanned.",
                self.south.display()
            );
            return Ok(());
        }
        let visibility = visibility(origin_url);
        if matches!(visibility.as_str(), "PRIVATE" | "INTERNAL") {
            return Ok(());
        }
        if let Some(config) = self.config.as_ref().filter(|_| !configured) {
            return Err(refuse(format!(
                "safe-push: ✗ the deny-list {} is missing although {} exists, so a {visibility} push cannot be scanned for private values; restore it in south; NOT pushed.",
                config.display(),
                self.south.display()
            )));
        }
        self.report(self.findings(root, base, head, tag), &visibility)
    }

    fn findings(&self, root: &Path, base: &str, head: &str, tag: &str) -> Result<BTreeSet<String>, String> {
        match &self.config {
            Some(config) => denylist::scan_range(root, config, base, head, Some(tag)),
            None => {
                let held = denylist::south_denylist(&self.south)?;
                denylist::scan_range(root, held.path(), base, head, Some(tag))
            }
        }
    }

    fn report(&self, findings: Result<BTreeSet<String>, String>, visibility: &str) -> Step {
        match findings {
            Ok(found) if found.is_empty() => return Ok(()),
            Ok(_) | Err(_) if cancelled().is_err() => return cancelled(),
            Ok(found) => found.iter().for_each(|finding| println!("denylist: {finding}")),
            Err(error) => {
                println!("denylist: cannot scan: {error}");
                return Err(refuse(format!(
                    "safe-push: ✗ the deny-list scan could not run (exit 2), so a {visibility} push was not checked for private values; NOT pushed."
                )));
            }
        }
        let source = self.config.as_ref().map_or_else(|| format!("in {} origin/main", self.south.display()), |config| config.display().to_string());
        Err(refuse(format!(
            "safe-push: ✗ PRIVATE VALUE for a {visibility} repository matched the deny-list {source} — it belongs in south; NOT pushed."
        )))
    }
}

fn visibility(origin_url: &str) -> String {
    let output = Command::new("gh")
        .args(["repo", "view", origin_url, "--json", "visibility", "-q", ".visibility"])
        .stderr(Stdio::null())
        .output();
    let observed = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_owned())
        .unwrap_or_default();
    if observed.is_empty() { "UNKNOWN".to_owned() } else { observed }
}

pub(crate) fn instruction_limits(root: &Path, reference: &str) -> Step {
    match super::instruction_limits::check(root, reference) {
        0 => Ok(()),
        code => Err(Refused(code)),
    }
}

pub(crate) fn exit_status(status: std::io::Result<std::process::ExitStatus>) -> Step {
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(Refused(exit_code(status))),
        Err(_) => Err(Refused(127)),
    }
}

#[cfg(test)]
#[path = "safe_push_guards_tests.rs"]
mod tests;
