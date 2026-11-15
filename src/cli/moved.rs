//! Verbs that folded under a domain noun: an unrecognized command naming one prints where it lives now.
use crate::error::NorthError;

const MOVED: &[(&[&str], &str, &[&str])] = &[
    (&["scoreboard"], "dash", &["--ready"]),
    (&["incident"], "ledger incident", &[]),
    (&["flow"], "ledger flow", &[]),
    (&["tempo"], "ledger tempo", &[]),
    (&["worker-ledger"], "ledger worker", &[]),
    (&["credentials"], "ops credentials", &[]),
    (&["github"], "ops github", &[]),
    (&["digest"], "fleet digest", &[]),
    (&["orders"], "fleet orders", &[]),
    (&["report"], "fleet report", &[]),
    (&["context"], "config context", &[]),
    (&["release"], "land release", &[]),
    (&["repo", "audit"], "lane audit", &[]),
    (&["split"], "code-health split", &[]),
    (&["warcraft-close"], "desktop warcraft close", &[]),
    (&["wc3-login-field"], "desktop warcraft login-field", &[]),
    (&["watch"], "ops watch", &[]),
    (&["strategy"], "ops strategy", &[]),
    (&["mutant"], "test mutant", &[]),
    (&["coverage"], "test coverage", &[]),
    (&["agents"], "config agents", &[]),
    (&["privacy"], "check privacy", &[]),
    (&["dispatch"], "fleet dispatch", &[]),
    (&["pace"], "fleet pace", &[]),
    (&["usage"], "fleet usage", &[]),
    (&["laptop"], "fleet laptop", &[]),
    (&["agent-capture"], "desktop capture", &[]),
    (&["private-desktop"], "desktop private", &[]),
    (&["brief"], "work brief", &[]),
    (&["review-items"], "work review", &[]),
    (&["grounding"], "graph ground", &[]),
    (&["operator"], "config operator", &[]),
    (&["agent-config-check"], "check harness", &[]),
    (&["land-abandon"], "land abandon", &[]),
    (&["citation-check"], "check citations", &[]),
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
    NorthError::Usage(format!("unrecognized {kind} '{}'\n\nUsage: north [COMMAND]\n\nRun 'north' to open the TUI, or 'north --help' for help.", arguments.join(" ")))
}

pub(super) fn hint(arguments: &[String]) -> Option<String> {
    let (words, path, dropped) = MOVED.iter().find(|(words, ..)| arguments.starts_with(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()))?;
    let kept: Vec<&str> = arguments[words.len()..].iter().map(String::as_str).filter(|word| !dropped.contains(word)).collect();
    Some(format!("did you mean: north {path}{}{}", if kept.is_empty() { "" } else { " " }, kept.join(" ")))
}
