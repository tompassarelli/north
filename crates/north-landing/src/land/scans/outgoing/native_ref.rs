use std::path::Path;

use crate::landing::safe_push_state::refuse;
use crate::landing::step::{Step, git_quiet};

const NATIVE_REFS: [&str; 4] = ["life#", "accounts#", "travel#", "ops#"];

pub fn names_native_item(text: &str) -> bool {
    text.lines().any(|line| {
        NATIVE_REFS.iter().any(|prefix| {
            line.match_indices(prefix).any(|(at, _)| {
                let bounded = line[..at].chars().next_back().is_none_or(|before| !(before.is_alphanumeric() || before == '_'));
                bounded && line[at + prefix.len()..].starts_with(|next: char| next.is_ascii_digit())
            })
        })
    })
}

pub fn native_ref_scan(dir: &Path, names: &[&str], revisions: &[String]) -> Step {
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
