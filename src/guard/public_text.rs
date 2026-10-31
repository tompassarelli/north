use super::gh_write;
use crate::hook::{HookInput, Verdict};
use crate::platform::gitleaks;
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
        |text| gitleaks::scan_text(&config, text).unwrap_or_default(),
        |repo, dir| is_private(repo, dir.or(cwd.as_deref())),
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

pub(super) fn decide_with(
    input: &HookInput,
    mut scan: impl FnMut(&str) -> BTreeSet<String>,
    mut private: impl FnMut(Option<&str>, Option<&str>) -> bool,
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
        let rules = scan(&bodies.join("\n"));
        if !rules.is_empty() && (write.public || !private(write.repo.as_deref(), write.dir.as_deref())) {
            let rules: Vec<_> = rules.into_iter().collect();
            return Verdict::Deny(format!(
                "No. This gh command's text matches the public deny-list rule {}, and GitHub text is public. Reword it without that detail, or record the detail privately.",
                rules.join(", ")
            ));
        }
    }
    Verdict::Allow
}

#[cfg(test)]
#[path = "public_text_tests.rs"]
mod tests;
