use std::path::Path;

use crate::fleet::model::Machine;

/// The home directory of the Unix user a session login logs in as, None when the login names no user or the user is the operator, whose own home the brief already names.
pub(super) fn login_home(login: &str, operator_home: &Path) -> Option<String> {
    let user = login.split_once('@').map(|(user, _)| user).filter(|user| !user.is_empty())?;
    let home = format!("/home/{user}");
    (Path::new(&home) != operator_home).then_some(home)
}

/// `brief` with the operator's code directory, `<operator_home>/code`, replaced by `<login_home>/code` wherever the path starts at a word boundary, so the paths it names are the repositories the agent user owns.
pub(super) fn rebase(brief: &str, operator_home: &Path, login_home: &str) -> String {
    let from = format!("{}/code", operator_home.display());
    let to = format!("{login_home}/code");
    let mut out = String::with_capacity(brief.len());
    let mut copied = 0;
    for (at, _) in brief.match_indices(&from) {
        let end = at + from.len();
        let starts_word = brief[..at].chars().next_back().is_none_or(|prev| !(prev.is_alphanumeric() || "_-./".contains(prev)));
        let ends_word = brief[end..].chars().next().is_none_or(|next| !(next.is_alphanumeric() || "_-.".contains(next)));
        if starts_word && ends_word {
            out.push_str(&brief[copied..at]);
            out.push_str(&to);
            copied = end;
        }
    }
    out.push_str(&brief[copied..]);
    out
}

pub(super) fn rebased(brief: &str, operator_home: &Path, machine: Option<&Machine>) -> Option<String> {
    let home = login_home(machine?.session_login(), operator_home)?;
    Some(rebase(brief, operator_home, &home))
}

pub(super) fn for_machine(brief: &str, machine: Option<&Machine>) -> Option<String> {
    let operator_home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    rebased(brief, Path::new(&operator_home), machine)
}

#[cfg(test)]
#[path = "agent_root_tests.rs"]
mod tests;
