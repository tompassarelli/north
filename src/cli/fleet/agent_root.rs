use std::path::Path;

/// The home directory of the Unix user an ssh destination logs in as, None when the destination names no user or the user is the operator, whose own home the brief already names.
pub(super) fn login_home(ssh: &str, operator_home: &Path) -> Option<String> {
    let user = ssh.split_once('@').map(|(user, _)| user).filter(|user| !user.is_empty())?;
    let home = format!("/home/{user}");
    (Path::new(&home) != operator_home).then_some(home)
}

/// `brief` with the operator's code directory, `<operator_home>/code`, replaced by `<login_home>/code`, so the paths it names are the repositories the agent user owns.
pub(super) fn rebase(brief: &str, operator_home: &Path, login_home: &str) -> String {
    let (from, to) = (format!("{}/code", operator_home.display()), format!("{login_home}/code"));
    let mut out = String::with_capacity(brief.len());
    let mut rest = brief;
    while let Some(at) = rest.find(&from) {
        let end = at + from.len();
        let whole = rest[end..].chars().next().is_none_or(|next| !(next.is_alphanumeric() || "_-.".contains(next)));
        out.push_str(&rest[..at]);
        out.push_str(if whole { &to } else { &from });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

pub(super) fn for_machine(brief: &str, ssh: Option<&str>) -> Option<String> {
    let operator_home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    let home = login_home(ssh?, Path::new(&operator_home))?;
    Some(rebase(brief, Path::new(&operator_home), &home))
}

#[cfg(test)]
#[path = "agent_root_tests.rs"]
mod tests;
