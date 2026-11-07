use std::path::Path;

use super::{LANDER_PATHS, Repository, Round, run_replay};
use crate::land::candidate::git;
use crate::land::{enqueue, replay, sheriff};
use crate::landing::red_check::Failure;

pub(super) struct Held<'a> {
    pub(super) binary: Option<&'a Path>,
    pub(super) only_fixes: bool,
}

fn first_word(verdict: &str) -> &str {
    verdict.split_whitespace().next().unwrap_or_default()
}

fn in_lander(checkout: &Path, base: &str, failure: &Failure) -> bool {
    let files = match failure {
        Failure::Harness(path) => vec![path.clone()],
        Failure::Cargo(name) => {
            let leaf = name.rsplit("::").next().unwrap_or(name);
            let found = git(checkout, &["grep", "-l", "-E", &format!(r"fn {leaf}\b"), base, "--", "*.rs"]).unwrap_or_default();
            found.lines().map(|line| line.split_once(':').map_or(line, |(_, path)| path).to_owned()).collect()
        }
        Failure::Line(_) | Failure::Step(_) => Vec::new(),
    };
    files.is_empty() || files.iter().any(|file| LANDER_PATHS.iter().any(|prefix| file.starts_with(prefix)))
}

pub(super) fn tolerated(held: &Held, repository: &Repository, checker: &str, round: &Round, base: &str, candidate: &replay::Replayed) -> Result<String, String> {
    let binary = held.binary.ok_or("no held runtime to compare with")?;
    let against = run_replay(binary, &repository.checkout, base, checker, &round.tickets())?;
    if first_word(&against.verdict) != first_word(&candidate.verdict) {
        return Err(format!("the held runtime replayed {}", first_word(&against.verdict)));
    }
    let new: Vec<&Failure> = candidate.failures.difference(&against.failures).collect();
    if new.is_empty() {
        return Ok("the held runtime replays it the same way: no new failure".to_owned());
    }
    let named = new.iter().map(|failure| failure.label()).collect::<Vec<_>>().join(", ");
    if held.only_fixes && !new.iter().any(|failure| in_lander(&repository.checkout, base, failure)) {
        return Ok(format!("a revert or sheriff-fix candidate whose new failures ({named}) lie outside the lander's crates"));
    }
    Err(format!("new failures the held runtime does not have: {named}"))
}

pub(super) fn only_fixes(checkout: &Path, held: &Path, target: &Path) -> bool {
    let name = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned());
    let (Some(held), Some(target)) = (name(held), name(target)) else { return false };
    let Ok(messages) = git(checkout, &["log", "--first-parent", "-z", "--format=%B", &format!("{held}..{target}")]) else { return false };
    let messages: Vec<&str> = messages.split('\0').map(str::trim).filter(|message| !message.is_empty()).collect();
    !messages.is_empty() && messages.iter().all(|message| sheriff::is_revert(message) || message.lines().any(enqueue::is_main_red_marker))
}

