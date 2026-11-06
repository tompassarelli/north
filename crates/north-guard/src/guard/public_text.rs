use super::gh_write::{self, GhWrite};
use crate::hook::{HookInput, Verdict};
use crate::platform::betterleaks;
use crate::platform::private_text::{self, Finding};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

const CONFIG_ENV: &str = "NORTH_PUBLIC_DENYLIST";
const CONFIG_UNDER_HOME: &str = "code/south/main/privacy/public-denylist.toml";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let Some(config) = config() else {
        return Verdict::Allow;
    };
    let cwd = input.cwd().map(str::to_owned);
    decide_with(
        input,
        |text| betterleaks::scan_text(&config, text).unwrap_or_default(),
        |repo, dir| is_private(repo, dir.or(cwd.as_deref())),
        private_text::judge_in_hook,
        current_body,
    )
}

fn config() -> Option<PathBuf> {
    let named = std::env::var_os(CONFIG_ENV)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(CONFIG_UNDER_HOME)))?;
    named.is_file().then_some(named)
}

fn is_private(repo: Option<&str>, cwd: Option<&str>) -> bool {
    let mut gh = Command::new("gh");
    gh.args(["repo", "view"]);
    gh.args(repo);
    gh.args(["--json", "isPrivate", "-q", ".isPrivate"]);
    if let Some(cwd) = cwd {
        gh.current_dir(cwd);
    }
    gh.output()
        .ok()
        .filter(|out| out.status.success())
        .is_some_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "true")
}

fn current_body(write: &GhWrite) -> Option<String> {
    let (group, number) = write.edits.as_ref()?;
    let mut gh = Command::new("gh");
    gh.args([group.as_str(), "view", number.as_str()]);
    if let Some(repo) = &write.repo {
        gh.args(["-R", repo]);
    }
    gh.args(["--json", "body", "-q", ".body"]);
    if let Some(dir) = &write.dir {
        gh.current_dir(dir);
    }
    gh.output().ok().filter(|out| out.status.success()).map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn added(text: &str, current: &str) -> String {
    let held: BTreeSet<&str> = current.lines().map(str::trim_end).collect();
    text.lines().filter(|line| !held.contains(line.trim_end())).map(|line| format!("{line}\n")).collect()
}

pub(super) fn decide_with(
    input: &HookInput,
    mut scan: impl FnMut(&str) -> BTreeSet<String>,
    mut private: impl FnMut(Option<&str>, Option<&str>) -> bool,
    mut judge: impl FnMut(&str) -> Result<Vec<Finding>, String>,
    mut current: impl FnMut(&GhWrite) -> Option<String>,
) -> Verdict {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    for write in gh_write::writes(&text.lossy(), input.cwd()) {
        let mut bodies = write.bodies.clone();
        if write.opaque {
            bodies.push(text.lossy().into_owned());
        }
        if bodies.is_empty() {
            continue;
        }
        let joined = bodies.join("\n");
        let rules = scan(&joined);
        let mut public = None;
        let mut is_public = || *public.get_or_insert_with(|| write.public || !private(write.repo.as_deref(), write.dir.as_deref()));
        if !rules.is_empty() && is_public() {
            let rules: Vec<_> = rules.into_iter().collect();
            return Verdict::Deny(format!(
                "No. This gh command's text matches the public deny-list rule {}, and GitHub text is public. Reword it without that detail, or record the detail privately.",
                rules.join(", ")
            ));
        }
        if !joined.trim().is_empty() && is_public() {
            let judged = current(&write).map_or_else(|| joined.clone(), |held| added(&joined, &held));
            match if judged.trim().is_empty() { Ok(Vec::new()) } else { judge(&judged) } {
                Ok(found) if found.is_empty() => {}
                Ok(found) => return Verdict::Deny(private_text::refusal(&found)),
                Err(reason) => return Verdict::Deny(private_text::unavailable(&reason)),
            }
        }
    }
    Verdict::Allow
}

#[cfg(test)]
#[path = "public_text_tests.rs"]
mod tests;
