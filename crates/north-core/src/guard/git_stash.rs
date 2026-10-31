use super::git_invocation::{argument_spans, bash_command, invocation, short_cluster, words};
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::sync::LazyLock;

const DENIAL: &str = "No. The stash is shared by every worktree, so another session can pop your changes. Commit a WIP commit instead (git commit -m wip PATHS), or `git stash push -u -m TAG` then `git stash apply <sha>`.";

static STASH: LazyLock<Regex> = LazyLock::new(|| invocation("stash", true));

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(input)
}

pub(super) fn decide(input: &HookInput) -> Verdict {
    let Some(command) = bash_command(input) else {
        return Verdict::Allow;
    };
    if argument_spans(&command, &STASH)
        .into_iter()
        .any(|arguments| unsafe_stash(&words(arguments).collect::<Vec<_>>()))
    {
        return Verdict::Deny(DENIAL.into());
    }
    Verdict::Allow
}

fn unsafe_stash(words: &[&str]) -> bool {
    match words.first().copied() {
        None | Some("pop") => true,
        Some("push" | "save") => !names_entry(&words[1..]),
        Some(option) if option.starts_with('-') => !names_entry(words),
        Some(_) => false,
    }
}

fn names_entry(words: &[&str]) -> bool {
    words.iter().any(|word| {
        matches!(*word, "-m" | "--message")
            || word.starts_with("--message=")
            || (word.starts_with("-m") && word.len() > 2)
            || (short_cluster(word) && word.ends_with('m'))
    })
}
