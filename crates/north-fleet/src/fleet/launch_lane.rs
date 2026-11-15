//! The lane check in front of a launch (north#1625): a brief whose `Item:` line names another repository than the
//! checkout the launch starts in makes every lane it works in the wrong repository, and `finish` refuses each of those
//! lanes only at the end, so the launch is refused here with the `--cwd` that names the right checkout.
use std::path::Path;

use north_base::platform::repo_names::checkout_name;
use north_foundation::error::{NorthError, NorthResult};

/// The checkout under a `code/` directory that `cwd` sits in: `HOME/code/NAME/main` and
/// `HOME/code/NAME/worktrees/LANE` both give NAME, and a directory outside one gives nothing.
pub fn checkout_in(cwd: &Path) -> Option<&str> {
    let names: Vec<&str> = cwd.components().filter_map(|component| component.as_os_str().to_str()).collect();
    let at = names.iter().rposition(|name| *name == "code")?;
    names.get(at + 1).copied()
}

/// The repository and number of the first `Item:` line of `prompt`, `north#1625` and `tompasarelli/north#1625` alike.
pub fn item(prompt: &str) -> Option<(&str, &str)> {
    let reference = prompt.lines().find_map(|line| line.trim_start().strip_prefix("Item:"))?.split_whitespace().next()?;
    let (repo, number) = reference.split_once('#')?;
    let repo = repo.rsplit('/').next().filter(|repo| !repo.is_empty())?;
    (!number.is_empty() && number.chars().all(|character| character.is_ascii_digit())).then_some((repo, number))
}

/// Why a launch of `prompt` in `cwd` may not start, or `None` when the brief names no item, `cwd` sits outside a
/// `code/` checkout, or both name one repository.
pub fn refusal(prompt: &str, cwd: &Path) -> Option<String> {
    let (repo, number) = item(prompt)?;
    let start = checkout_in(cwd)?;
    let wanted = checkout_name(repo);
    if start == wanted {
        return None;
    }
    let container = cwd.ancestors().find(|path| path.file_name().is_some_and(|name| name == "code"))?;
    let lane = container.join(wanted).join("main");
    Some(format!(
        "Item: {repo}#{number} names {wanted}'s repository, but this launch starts in {start}'s checkout {}, so every lane it makes there is {start}'s and finish refuses them; launch it with --cwd {} so its lane is {wanted}'s",
        cwd.display(),
        lane.display()
    ))
}

/// `refusal` as the launch's error.
pub fn check(prompt: &str, cwd: &Path) -> NorthResult<()> {
    refusal(prompt, cwd).map_or(Ok(()), |why| Err(NorthError::Refused(why)))
}

#[cfg(test)]
#[path = "launch_lane_tests.rs"]
mod tests;
