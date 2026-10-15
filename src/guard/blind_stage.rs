use super::git_invocation::{argument_spans, bash_command, invocation, short_cluster, words};
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::sync::LazyLock;

static ADD: LazyLock<Regex> = LazyLock::new(|| invocation("add", false));
static COMMIT: LazyLock<Regex> = LazyLock::new(|| invocation("commit", false));

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(input)
}

pub(super) fn decide(input: &HookInput) -> Verdict {
    let Some(command) = bash_command(input) else {
        return Verdict::Allow;
    };
    let stages_all = |pattern: &Regex, blind: fn(&str) -> bool| {
        argument_spans(&command, pattern)
            .into_iter()
            .any(|arguments| words(arguments).any(blind))
    };
    let hit = if stages_all(&ADD, blind_add) {
        "git add"
    } else if stages_all(&COMMIT, blind_commit) {
        "git commit -a"
    } else {
        return Verdict::Allow;
    };
    Verdict::Deny(format!(
        "BLOCKED: blind staging refused ({hit}) — enumerate the paths you intend to commit instead, e.g. `git add path/to/file` or `git commit path/to/file -m '...'`. A commit MESSAGE or heredoc body that merely mentions the phrase is unaffected — only a real command-position invocation is denied. Rare explicit override: `north config agents off north-hook` (persistent, live), or a session LAUNCHED with AGENT_NO_AUTHORING_HOOKS=1."
    ))
}

fn blind_add(word: &str) -> bool {
    matches!(
        word,
        "-A" | "--all" | "-u" | "--update" | "--no-ignore-removal" | "."
    ) || (short_cluster(word) && word[1..].contains(['A', 'u']))
}

fn blind_commit(word: &str) -> bool {
    matches!(word, "-a" | "--all") || (short_cluster(word) && word[1..].contains(['a', 'A']))
}
