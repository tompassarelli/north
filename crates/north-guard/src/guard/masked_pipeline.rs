use crate::hook_input::{HookInput, Verdict};
use north_base::shell::{basename, shell_segments_sep, shlex_split, strip_heredoc_bodies};
use regex::Regex;
use std::sync::LazyLock;

const CHECKS: &[&str] = &[
    "cargo", "cargo-nextest", "bun", "bunx", "npm", "npx", "pnpm", "yarn", "make", "just", "nix", "go", "pytest", "zig",
];
const FILTERS: &[&str] = &["tail", "head", "grep", "rg"];

static DESCRIPTOR_DUPLICATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d*>&(\d+|-)").unwrap());

pub fn decide(input: &HookInput) -> Verdict {
    if !input.is_shell() {
        return Verdict::Allow;
    }
    let Some(command) = input.command() else {
        return Verdict::Allow;
    };
    match masked_publish(command) {
        Some((check, publish)) => Verdict::Deny(denial(command, &check, &publish)),
        None => Verdict::Allow,
    }
}

fn masked_publish(command: &str) -> Option<(String, String)> {
    let text = strip_heredoc_bodies(command);
    if text.contains("pipefail") {
        return None;
    }
    let text = DESCRIPTOR_DUPLICATION.replace_all(&text, "");
    let mut stages: Vec<String> = Vec::new();
    let mut masked: Option<String> = None;
    for (segment, separator) in shell_segments_sep(&text) {
        stages.push(segment);
        if separator.as_deref() == Some("|") {
            continue;
        }
        let pipeline = std::mem::take(&mut stages);
        if let Some(check) = &masked
            && publishes(&words(&pipeline[0]))
        {
            return Some((check.clone(), pipeline.join(" | ")));
        }
        if masks_check(&pipeline) {
            masked = Some(pipeline.join(" | "));
        }
        if separator.as_deref() != Some("&&") {
            masked = None;
        }
    }
    None
}

fn words(stage: &str) -> Vec<String> {
    shlex_split(stage).unwrap_or_else(|| stage.split_whitespace().map(str::to_owned).collect())
}

fn masks_check(pipeline: &[String]) -> bool {
    pipeline.len() > 1
        && words(&pipeline[0]).iter().any(|word| CHECKS.contains(&basename(word)))
        && pipeline[1..].iter().any(|stage| words(stage).first().is_some_and(|word| FILTERS.contains(&basename(word))))
}

fn publishes(words: &[String]) -> bool {
    let named = |at: usize, name: &str| words.get(at).is_some_and(|word| word == name);
    words.iter().enumerate().any(|(at, word)| match basename(word) {
        "safe-push" => true,
        "north" => named(at + 1, "land") && named(at + 2, "enqueue"),
        "git" => words[at + 1..].iter().any(|word| word == "commit"),
        _ => false,
    })
}

fn denial(command: &str, check: &str, publish: &str) -> String {
    format!(
        "masked-pipeline: `{check}` exits with the status of its last stage, so `&&` runs `{publish}` even when the check fails. Turn on pipefail first: `set -o pipefail; {command}`"
    )
}
