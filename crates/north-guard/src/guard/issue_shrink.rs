use super::gh_write;
use super::issue_labels::gh_program;
use crate::hook_input::{HookInput, Verdict};
use regex::Regex;
use std::sync::LazyLock;

const OWNER: &str = "tompassarelli/";
static ALLOWED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"--allow-shrink\s+\S").unwrap());

trait World {
    fn repo(&mut self, named: Option<&str>, dir: Option<&str>) -> Option<String>;
    fn body(&mut self, repo: &str, number: u64) -> Option<String>;
}

struct Live {
    program: String,
}

impl World for Live {
    fn repo(&mut self, named: Option<&str>, dir: Option<&str>) -> Option<String> {
        match named {
            Some(repo) => Some(repo.to_owned()),
            None => gh_program(&self.program, dir, &["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]).map(|out| out.trim().to_owned()),
        }
    }

    fn body(&mut self, repo: &str, number: u64) -> Option<String> {
        gh_program(&self.program, None, &["issue", "view", &number.to_string(), "-R", repo, "--json", "body", "-q", ".body"])
    }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    refuse(input, &mut Live { program: "gh".to_owned() })
}

fn refuse(input: &HookInput, world: &mut impl World) -> Verdict {
    if !input.is_shell() || !input.raw.windows(5).any(|bytes| bytes == b"issue") {
        return Verdict::Allow;
    }
    let Some(text) = input.command() else {
        return Verdict::Allow;
    };
    if ALLOWED.is_match(text) {
        return Verdict::Allow;
    }
    for write in gh_write::writes(text, input.cwd()) {
        let Some((_, number)) = write.edits.as_ref().filter(|(group, _)| group == "issue") else {
            continue;
        };
        let Ok(number) = number.parse::<u64>() else {
            continue;
        };
        if !write.body_set || write.opaque || write.unread || written_here(text, &write) {
            continue;
        }
        let Some(repo) = world.repo(write.repo.as_deref(), write.dir.as_deref()).filter(|repo| repo.to_lowercase().starts_with(OWNER)) else {
            continue;
        };
        let Some(current) = world.body(&repo, number) else {
            continue;
        };
        let new = write.bodies.iter().filter(|part| write.title.as_deref() != Some(part.as_str())).cloned().collect::<Vec<_>>().join("\n");
        if super::gh_screen::shrinks(&current, &new) {
            let (old, new) = (current.trim().chars().count(), new.trim().chars().count());
            return Verdict::Deny(format!(
                "No. This edit replaces {repo}#{number}'s {old}-character body with {new} characters, which blanks or drops most of it. Append to the body you read (`gh issue view {number} -R {repo} --json body -q .body > /tmp/{number}.md`, then add your lines) or comment instead; when the shrink is meant, pass `--allow-shrink REASON` to `gh issue edit`."
            ));
        }
    }
    Verdict::Allow
}

/// A body file the same command redirects into is not on disk yet, so what is there now is not what `gh` will read.
fn written_here(text: &str, write: &gh_write::GhWrite) -> bool {
    write.files.iter().filter_map(|file| file.file_name()).any(|name| {
        let pattern = format!(r"(?:>|\btee\b)[^\n;&|]*{}", regex::escape(&name.to_string_lossy()));
        Regex::new(&pattern).is_ok_and(|found| found.is_match(text))
    })
}

#[cfg(test)]
#[path = "issue_shrink_tests.rs"]
mod tests;
