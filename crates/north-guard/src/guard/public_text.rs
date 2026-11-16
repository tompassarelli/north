use super::gh_write;
use crate::hook_input::{HookInput, Verdict};
use north_base::platform::betterleaks;
use north_base::platform::shared_config;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

const CONFIG_ENV: &str = "NORTH_PUBLIC_DENYLIST";
const ACCOUNT_REFUSAL: &str = "No. This gh command's text names a signed-in account by its sign-in email, and GitHub text in a public repository names an account only by its provider. Reword it as a Claude account.";

pub fn evaluate(input: &HookInput) -> Verdict {
    let Some(config) = config() else {
        return Verdict::Allow;
    };
    let cwd = input.cwd().map(str::to_owned);
    decide_with(
        input,
        |text| betterleaks::scan_text(&config, text).unwrap_or_default(),
        |repo, dir| is_private(repo, dir.or(cwd.as_deref())),
        north_base::profiles::signed_in_here,
    )
}

fn config() -> Option<PathBuf> {
    let named = std::env::var_os(CONFIG_ENV)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| shared_config::checkout(&PathBuf::from(home)).join("privacy/public-denylist.toml")))?;
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

fn names_account(text: &str, accounts: &mut impl FnMut() -> Vec<String>) -> bool {
    let text = text.to_lowercase();
    accounts().iter().any(|email| text.contains(&email.to_lowercase()))
}

pub(super) fn decide_with(
    input: &HookInput,
    mut scan: impl FnMut(&str) -> BTreeSet<String>,
    mut private: impl FnMut(Option<&str>, Option<&str>) -> bool,
    mut accounts: impl FnMut() -> Vec<String>,
) -> Verdict {
    if !input.is_shell() || !input.raw.windows(2).any(|bytes| bytes == b"gh") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    for write in gh_write::writes(text, input.cwd()) {
        let mut bodies = write.bodies.clone();
        if write.opaque {
            bodies.push(text.to_owned());
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
        if is_public() && names_account(&joined, &mut accounts) {
            return Verdict::Deny(ACCOUNT_REFUSAL.to_owned());
        }
    }
    Verdict::Allow
}

#[cfg(test)]
#[path = "public_text_tests.rs"]
mod tests;
