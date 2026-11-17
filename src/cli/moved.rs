//! Verbs that folded under a domain noun: an unrecognized command naming one prints where it lives now.
use crate::error::NorthError;

const MOVED: &[(&[&str], &str, &[&str])] = &[
    (&["credentials"], "ops credentials", &[]),
    (&["github"], "ops github", &[]),
    (&["release"], "land release", &[]),
    (&["split"], "code-health split", &[]),
    (&["watch"], "ops watch", &[]),
    (&["mutant"], "test mutant", &[]),
    (&["coverage"], "test coverage", &[]),
    (&["agents"], "config agents", &[]),
    (&["privacy"], "check disclosure", &[]),
    (&["operator"], "config operator", &[]),
    (&["agent-config-check"], "check harness", &[]),
    (&["land-abandon"], "land abandon", &[]),
    (&["issue-box-lint"], "check issue-boxes", &[]),
    (&["lane-gc"], "lane gc", &[]),
    (&["lane-layout"], "lane layout", &[]),
    (&["lane-reap"], "lane reap", &[]),
    (&["lane-reclaim"], "lane reclaim", &[]),
    (&["lane-rescue"], "lane rescue", &[]),
    (&["lane-seed"], "lane seed", &[]),
    (&["lane-squash"], "lane squash", &[]),
];

pub(super) fn refusal(arguments: &[String]) -> NorthError {
    if let Some(hint) = hint(arguments) {
        return NorthError::Usage(format!("unrecognized command '{}'\n\n{hint}", arguments.join(" ")));
    }
    let kind = if arguments[0].starts_with('-') { "option" } else { "command" };
    NorthError::Usage(format!("unrecognized {kind} '{}'\n\nUsage: north [COMMAND]\n\nRun 'north --help' for help.", arguments.join(" ")))
}

pub(super) fn hint(arguments: &[String]) -> Option<String> {
    let (words, path, dropped) = MOVED.iter().find(|(words, ..)| arguments.starts_with(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()))?;
    let kept: Vec<&str> = arguments[words.len()..].iter().map(String::as_str).filter(|word| !dropped.contains(word)).collect();
    Some(format!("did you mean: north {path}{}{}", if kept.is_empty() { "" } else { " " }, kept.join(" ")))
}
